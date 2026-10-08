Prepared only; not executed or accepted. Use a future release containing previewLOD.
Never run against or close the human instance already open. Launch a new instance only after coordination, with
TRANSCRIPTOR_CONFIG_DIR=C:\Users\gabri\Todo\transcriber-v2\implementation\evidence\e2\continuacion-14\preview-prepared-20260930-161534\config
TRANSCRIPTOR_CACHE_DIR=C:\Users\gabri\Todo\transcriber-v2\implementation\evidence\e2\continuacion-14\preview-prepared-20260930-161534\cache
TRANSCRIPTOR_LOGS_DIR=C:\Users\gabri\Todo\transcriber-v2\implementation\evidence\e2\continuacion-14\preview-prepared-20260930-161534\logs
Transcriptor.exe --script C:\Users\gabri\Todo\transcriber-v2\implementation\evidence\e2\continuacion-14\preview-prepared-20260930-161534\preview-script.json

This invented input has1000clips/20000items, with10000layer0 ranges[0;6) so a middle body hit can drag all selected items.
At PHASE1_READY, drag the middle of layer0 slightly right (e.g.0.25s), avoiding handles, and KEEP the mouse pressed.
Coordinator creates capture-commit.signal as an empty file here. Hold until preview-commit.png is written.
Then release the mouse, wait for one committed change, and coordinator creates released.signal.
Script asserts revision1, runs undo/redo to revisions2/3, and enters PHASE2_READY.
Drag again and hold; coordinator creates capture-cancel.signal and waits for preview-cancel.png.
Press Escape, release, and coordinator creates cancelled.signal. Revision must stay3.
Compare full items/ranges in after-redo.json vs after-cancel.json; no cancellation mutation is permitted.
Every signal is local/non-secret. Timeouts fail; a screenshot file alone is not acceptance.
No script hook injects pointer events or calls preview: real human input is required. View/layout visibility remains unverified.
