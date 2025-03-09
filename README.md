# Bilibili

This is a command line tool to convert local cached video contents by the official Bilibili client.
You will need to cache the video contents that wants to convert from the official client before running this tool.

## Installation

```
cargo install bilibili
```

## Compatibility

Tested on macos only.
The default cache directory is under `/Users/<user>/Movies/bilibili` and the output directory
is hardcoded to `/Users/<user>/Movies/output`.

The ``<user>`` is determined from the ``HOME`` environment variable.
