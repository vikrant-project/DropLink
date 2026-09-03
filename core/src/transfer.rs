use crate::protocol::{TransferManifest, TransferProgress, TransferStatus};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Sliding window speed tracker for smooth, jitter-free MB/s and ETA metrics.
pub struct SpeedTracker {
    history: VecDeque<(Instant, u64)>, // (timestamp, cumulative_bytes)
    window_duration: Duration,
}

impl SpeedTracker {
    pub fn new(window_seconds: u64) -> Self {
        Self {
            history: VecDeque::new(),
            window_duration: Duration::from_secs(window_seconds),
        }
    }

    pub fn record_bytes(&mut self, total_bytes: u64) {
        let now = Instant::now();
        self.history.push_back((now, total_bytes));

        // Purge samples older than window
        while let Some(&(time, _)) = self.history.front() {
            if now.duration_since(time) > self.window_duration {
                self.history.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn current_speed_bytes_per_sec(&self) -> f64 {
        if self.history.len() < 2 {
            return 0.0;
        }

        let (first_time, first_bytes) = self.history.front().unwrap();
        let (last_time, last_bytes) = self.history.back().unwrap();

        let elapsed = last_time.duration_since(*first_time).as_secs_f64();
        if elapsed <= 0.001 {
            return 0.0;
        }

        let bytes_delta = last_bytes.saturating_sub(*first_bytes) as f64;
        bytes_delta / elapsed
    }

    pub fn estimate_remaining_seconds(&self, remaining_bytes: u64) -> Option<u64> {
        let speed = self.current_speed_bytes_per_sec();
        if speed > 1024.0 { // Minimum 1 KB/s to give meaningful ETA
            Some((remaining_bytes as f64 / speed).round() as u64)
        } else {
            None
        }
    }
}

/// Active transfer session controller with pause, cancel, and progress hooks.
pub struct ActiveTransferSession {
    pub session_id: Uuid,
    pub manifest: TransferManifest,
    pub is_cancelled: Arc<AtomicBool>,
    pub is_paused: Arc<AtomicBool>,
    pub speed_tracker: SpeedTracker,
    pub status: TransferStatus,
    pub current_file_index: usize,
    pub current_file_bytes: u64,
    pub total_bytes_transferred: u64,
}

impl ActiveTransferSession {
    pub fn new(manifest: TransferManifest) -> Self {
        Self {
            session_id: manifest.session_id,
            manifest,
            is_cancelled: Arc::new(AtomicBool::new(false)),
            is_paused: Arc::new(AtomicBool::new(false)),
            speed_tracker: SpeedTracker::new(2),
            status: TransferStatus::Pending,
            current_file_index: 0,
            current_file_bytes: 0,
            total_bytes_transferred: 0,
        }
    }

    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::Relaxed)
    }

    pub fn toggle_pause(&self) -> bool {
        let current = self.is_paused.load(Ordering::Relaxed);
        let next = !current;
        self.is_paused.store(next, Ordering::SeqCst);
        next
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    pub fn update_progress(&mut self, current_file_bytes: u64, cumulative_transferred: u64) {
        self.current_file_bytes = current_file_bytes;
        self.total_bytes_transferred = cumulative_transferred;
        self.speed_tracker.record_bytes(cumulative_transferred);
    }

    pub fn snapshot_progress(&self, error_message: Option<String>) -> TransferProgress {
        let current_file = self.manifest.files.get(self.current_file_index);
        let file_name = current_file.map(|f| f.name.clone()).unwrap_or_default();
        let file_total = current_file.map(|f| f.size).unwrap_or(0);

        let speed = self.speed_tracker.current_speed_bytes_per_sec();
        let remaining_bytes = self.manifest.total_size.saturating_sub(self.total_bytes_transferred);
        let eta = self.speed_tracker.estimate_remaining_seconds(remaining_bytes);

        TransferProgress {
            session_id: self.session_id,
            status: self.status,
            current_file_index: self.current_file_index,
            current_file_name: file_name,
            current_file_bytes: self.current_file_bytes,
            current_file_total: file_total,
            total_bytes_transferred: self.total_bytes_transferred,
            total_bytes_overall: self.manifest.total_size,
            speed_bytes_per_sec: speed,
            estimated_seconds_remaining: eta,
            error_message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speed_tracker() {
        let mut tracker = SpeedTracker::new(2);
        tracker.record_bytes(0);
        std::thread::sleep(Duration::from_millis(50));
        tracker.record_bytes(1024 * 1024); // 1 MB in 50ms

        let speed = tracker.current_speed_bytes_per_sec();
        assert!(speed > 0.0);

        let eta = tracker.estimate_remaining_seconds(5 * 1024 * 1024);
        assert!(eta.is_some());
    }
}
