"""Run the offscreen workspace fixture without modifying examples or opening windows."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--release", action="store_true")
parser.add_argument("--output", required=True, type=Path)
args = parser.parse_args()
source_dir = Path(__file__).resolve().parent
repo = source_dir.parents[2]
args.output.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix="rxui-scroll-check-") as directory:
    probe = Path(directory)
    source = (source_dir / "offscreen.rs").read_text().replace(
        '"SOURCE_FONT_PATH"', json.dumps(str(repo / "crates/rxui/tests/fonts/SourceSans3-Regular.otf"))
    )
    manifest = (source_dir / "manifest.toml").read_text().replace(
        '"RXUI_CRATE_PATH"', json.dumps(str(repo / "crates/rxui"))
    )
    (probe / "offscreen.rs").write_text(source)
    (probe / "Cargo.toml").write_text(manifest)
    shutil.copy2(repo / "Cargo.lock", probe / "Cargo.lock")
    env = {**os.environ, "CARGO_INCREMENTAL": "0", "CARGO_TARGET_DIR": str(repo / "target")}
    command = ["cargo", "build", "--offline", "--manifest-path", str(probe / "Cargo.toml")]
    if args.release:
        command.append("--release")
    subprocess.run(command, env=env, check=True)
    binary = repo / "target" / ("release" if args.release else "debug") / "rxui-scroll-offscreen"
    for run in range(1, 4):
        with (args.output / f"run-{run}.csv").open("w") as output:
            subprocess.run([str(binary)], stdout=output, check=True)
