PREPARED ONLY: technical LLM montage fixture, no ASR/real speech alignment or human quality acceptance.
Copied exact previously imported V2 demo video/master/project/audit/history/bundles; all original files intact.
asset.path resolves from fixture.transcriptor parent to its own media/fixture-a.mp4 copy.
Master words/acoustic values are explicitly invented demo fixture values, not results transferred from other media.
No new Rust importer is required: original import used normal native ImportV1 commands; current Rust host
must load the copied ProjectStore, validate source bundles and prepare a fresh montage request normally.
Conservative stdlib sizing is not a bound request; tokenizer4096 cap still requires the real host check.
After coordinating the ML window, parent may run:
.local/e5-editorial-venv/Scripts/python.exe tests/scripts/run_editorial_host_owned.py .local/e5-editorial-montage-host-01 C:\Users\gabri\Todo\transcriber-v2\.local\editorial-montage-video-fixture-01\fixture.transcriptor --kind montage
