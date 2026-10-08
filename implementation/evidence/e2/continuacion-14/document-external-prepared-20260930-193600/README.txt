PREPARED ONLY: no native acceptance yet. Do not run old build349df or touch human PID6868.
After root authorizes a compiled new build, execute:
python tests/scripts/accept_e3_document_external_native.py --run C:\Users\gabri\Todo\transcriber-v2\implementation\evidence\e2\continuacion-14\document-external-prepared-20260930-193600 --exe NEW_ABSOLUTE_EXE --sha256 NEW_HASH
Runner probes its copied medium through native V2, rebases only copied synthetic documents to actual fingerprint,
then performs new-folder export, rejects repeated destination without changing bytes, waits for real monitor,
applies one synthetic project-name change (human_review defaults false), Undo/Redo, Save/reopen and reimport export.
No forced watcher polling, direct session commit, V1 execution, ML, physical playback or human GUI control.
Full field-conflict choices, stale-diff Apply, V1-linked document watcher and permission UX remain separate cases.
