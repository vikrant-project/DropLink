package com.droplink.app

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.os.Build

class DropLinkApp : Application() {

    companion object {
        const val CHANNEL_TRANSFERS = "droplink_transfers"
        const val CHANNEL_INCOMING = "droplink_incoming"
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannels()
    }

    private fun createNotificationChannels() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val transferChannel = NotificationChannel(
                CHANNEL_TRANSFERS,
                "DropLink File Transfers",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Shows live file transfer progress"
            }

            val incomingChannel = NotificationChannel(
                CHANNEL_INCOMING,
                "DropLink Incoming Requests",
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = "Alerts when a nearby device wants to send you files"
            }

            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(transferChannel)
            manager.createNotificationChannel(incomingChannel)
        }
    }
}
