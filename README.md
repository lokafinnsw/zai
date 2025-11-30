# Z.ai Coding Agent

AI-powered coding assistant using Z.ai's GLM-4.6 model.

## Features

- 🚀 200K context window for large codebases
- 💡 Code explanation and documentation
- 🔧 Smart refactoring suggestions
- 🐛 Automatic bug detection and fixes
- ⚡ Fast responses with GLM-4.5-Air option

## Quick Start

1. Get your API key at [z.ai/subscribe](https://z.ai/subscribe)
2. Install the CLI
3. Enter your API key when prompted
4. Start coding with AI!

## Installation

### From Source

```bash
git clone https://github.com/z-ai/zai-coding-agent
cd zai-coding-agent
cargo build --release
```

### From Binary

Download the latest release from [GitHub Releases](https://github.com/z-ai/zai-coding-agent/releases).

## Usage

### Interactive Mode

```bash
zai
```

### Single Query

```bash
zai "How do I implement a binary search in JavaScript?"
```

### Configuration

```bash
zai config              # Show current configuration
zai config --model      # Change model
zai config --key        # Change API key
```

## Models

| Model | Context | Best For |
|-------|---------|----------|
| GLM-4.6 | 200K | Complex coding tasks |
| GLM-4.5 | 128K | General use |
| GLM-4.5-Air | 128K | Quick responses |

## Attribution

This project is based on [Goose](https://github.com/block/goose) by Block, Inc.
Licensed under Apache 2.0.

## License

Apache 2.0
