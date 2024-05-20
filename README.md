# bul

[![ci](https://github.com/ynqa/bul/actions/workflows/ci.yml/badge.svg)](https://github.com/ynqa/bul/actions/workflows/ci.yml)

*bul* provides an interactive TUI to explore container logs for Kubernetes.

<img src="https://github.com/ynqa/ynqa/blob/master/demo/bul.gif">

## Features

- Filter streaming container logs based on keywords
  - (currently) Not offer search functionality at the level of regular expressions, grep or fuzzy search
  - Extracts logs that match a specific word by
    [contains](https://doc.rust-lang.org/std/string/struct.String.html#method.contains)
- Digger mode
  - Enable querying the latest N logs when switching to the mode
- Reconnect to log API
  - Allows users to control when to reconnect
- Flow control that determines how many logs are rendered within a certain period

> [!IMPORTANT]
> Please note that *bul* is still at a conceptual stage and in early development.
> Future updates may significantly alter its search capabilities and user interface.

## Installation

### Homebrew

```bash
brew install ynqa/tap/bul
```

### Cargo

```bash
cargo install bul
```

## Motivation

I frequently utilize `kubectl logs` or [stern](https://github.com/stern/stern)
to analyze errors or debug applications by examining the logs of Kubernetes Pods.

For example:

```bash
kubectl logs -n my-namespace my-pod | grep "something"
# Or
stern pod-query | grep "something"
```

Typically, when analyzing logs, these commands are used in conjunction with `grep`
to filter for specific keywords.
However, this process requires repeatedly running the command with adjusted parameters,
and re-running the command each time can be cumbersome.

To address this issue, *bul* project provides to allow users
to filter and review logs in real-time.
This design enables dynamic adjustment of filtering criteria
without the need to rerun the command.

## Keymap

| Key                  | Action
| :-                   | :-
| <kbd>Ctrl + C</kbd>  | Exit `bul`
| <kbd>Ctrl + F</kbd>  | Enter digger mode
| <kbd>Ctrl + R</kbd>  | Reconnect to log API
| <kbd>←</kbd>         | Move the cursor one character to the left
| <kbd>→</kbd>         | Move the cursor one character to the right
| <kbd>Ctrl + A</kbd>  | Move the cursor to the start of the filter
| <kbd>Ctrl + E</kbd>  | Move the cursor to the end of the filter
| <kbd>Backspace</kbd> | Delete a character of filter at the cursor position
| <kbd>Ctrl + U</kbd>  | Delete all characters of filter

## Usage

```bash
Interactive Kubernetes log viewer

Usage: bul [OPTIONS]

Options:
      --context <CONTEXT>
          Kubernetes context.
  -n, --namespace <NAMESPACE>
          Kubernetes namespace.
  -p, --pod-query <POD_QUERY>
          query to filter Pods.
      --container-states <CONTAINER_STATUS>
          Container states to filter containers. [default: all] [possible values: all, running, terminated, waiting]
      --log-retrieval-timeout <LOG_RETRIEVAL_TIMEOUT_MILLIS>
          Timeout to read a next line from the log stream in milliseconds. [default: 100]
      --render-interval <RENDER_INTERVAL_MILLIS>
          Interval to render a log line in milliseconds. [default: 10]
  -q, --queue-capacity <QUEUE_CAPACITY>
          Queue capacity to store the logs. [default: 1000]
  -h, --help
          Print help (see more with '--help')
```
