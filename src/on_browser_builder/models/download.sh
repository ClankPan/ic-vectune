SCRIPT_DIR=$(cd $(dirname $0); pwd)

curl -L -o "$SCRIPT_DIR/model.safetensors" 'https://huggingface.co/intfloat/e5-small-v2/resolve/main/model.safetensors'
curl -L -o "$SCRIPT_DIR/config.json" 'https://huggingface.co/intfloat/e5-small-v2/resolve/main/config.json'
curl -L -o "$SCRIPT_DIR/tokenizer.json" 'https://huggingface.co/intfloat/e5-small-v2/resolve/main/tokenizer.json'