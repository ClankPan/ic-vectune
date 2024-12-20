#!/bin/bash
set -e  # stop script if error happens

cargo build --target wasm32-unknown-unknown --release -p instance

wasm2wat target/wasm32-unknown-unknown/release/instance.wasm -o target/wasm32-unknown-unknown/release/instance.wat

grep '__wbindgen_placeholder__' target/wasm32-unknown-unknown/release/instance.wat