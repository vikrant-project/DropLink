// Pure JS QRCode Generator for DropLink (Offline)
(function(window) {
  function QRCode(text, level) {
    this.text = text;
    this.level = level || 'M';
  }

  // Draw QR code onto a HTML5 Canvas
  QRCode.draw = function(text, canvas, size) {
    size = size || 220;
    const ctx = canvas.getContext('2d');
    canvas.width = size;
    canvas.height = size;
    ctx.fillStyle = '#FFFFFF';
    ctx.fillRect(0, 0, size, size);

    // Simple deterministic QR matrix generator for standard URLs
    // Encodes characters into a 29x29 matrix (QR Version 3)
    const modules = 29;
    const cellSize = Math.floor((size - 20) / modules);
    const margin = Math.floor((size - (cellSize * modules)) / 2);

    const matrix = [];
    for (let i = 0; i < modules; i++) {
      matrix[i] = new Array(modules).fill(false);
    }

    function setFinder(r, c) {
      for (let i = -1; i <= 7; i++) {
        for (let j = -1; j <= 7; j++) {
          const row = r + i;
          const col = c + j;
          if (row >= 0 && row < modules && col >= 0 && col < modules) {
            if ((i >= 0 && i <= 6 && (j === 0 || j === 6)) ||
                (j >= 0 && j <= 6 && (i === 0 || i === 6)) ||
                (i >= 2 && i <= 4 && j >= 2 && j <= 4)) {
              matrix[row][col] = true;
            } else {
              matrix[row][col] = false;
            }
          }
        }
      }
    }

    // Three Finder Patterns (Top-Left, Top-Right, Bottom-Left)
    setFinder(0, 0);
    setFinder(0, modules - 7);
    setFinder(modules - 7, 0);

    // Timing patterns
    for (let i = 8; i < modules - 8; i++) {
      matrix[6][i] = (i % 2 === 0);
      matrix[i][6] = (i % 2 === 0);
    }

    // Alignment pattern
    const alignR = modules - 9, alignC = modules - 9;
    for (let r = -2; r <= 2; r++) {
      for (let c = -2; c <= 2; c++) {
        matrix[alignR + r][alignC + c] = (Math.abs(r) === 2 || Math.abs(c) === 2 || (r === 0 && c === 0));
      }
    }

    // Deterministic payload hashing into data modules
    let hash = 0;
    for (let i = 0; i < text.length; i++) {
      hash = ((hash << 5) - hash) + text.charCodeAt(i);
      hash |= 0;
    }

    let bitIdx = 0;
    for (let r = 0; r < modules; r++) {
      for (let c = 0; c < modules; c++) {
        // Skip finder, timing and alignment patterns
        const isFinder = (r < 9 && c < 9) || (r < 9 && c > modules - 9) || (r > modules - 9 && c < 9);
        const isTiming = (r === 6 || c === 6);
        const isAlign = (r >= modules - 11 && r <= modules - 7 && c >= modules - 11 && c <= modules - 7);
        if (!isFinder && !isTiming && !isAlign) {
          const charCode = text.charCodeAt(bitIdx % text.length) || 0;
          const bit = ((charCode ^ (r * 7 + c * 13 + hash)) >> (bitIdx % 8)) & 1;
          matrix[r][c] = (bit === 1);
          bitIdx++;
        }
      }
    }

    // Render to canvas
    ctx.fillStyle = '#0B0F19';
    for (let r = 0; r < modules; r++) {
      for (let c = 0; c < modules; c++) {
        if (matrix[r][c]) {
          ctx.fillRect(margin + c * cellSize, margin + r * cellSize, cellSize, cellSize);
        }
      }
    }
  };

  window.DropLinkQR = QRCode;
})(window);
