# parqeye

[![CI][actions-badge]][actions-url]

[actions-badge]: https://github.com/kaushiksrini/parqeye/actions/workflows/ci.yaml/badge.svg
[actions-url]: https://github.com/kaushiksrini/parqeye/actions/workflows/ci.yaml

`parqeye` lets you _peek inside_ your Parquet files. Instantly inspect their contents, schema, and metadata — right from your terminal.

![Demo](.github/assets/demo.gif)

**Features**

- **Interactive Data Visualization** - Browse through your Parquet data in a table view with keyboard navigation.
- **Schema Explorer** - Inspect column types, nested structures, and field definitions.
- **File Metadata** - View Parquet file-level metadata including version, created by, encoding stats and more.
- **Row Group Statistics** - Examine row group-level metadata, statistics, and data distribution across groups.
- **Tab-based Interface** - Quickly switch between Visualize, Schema, Metadata, and Row Groups views.
- **Terminal-native** - Works directly in your terminal.

# Usage

Run `parqeye` by providing the path to the `.parquet` file.

```
parqeye <path-to-parquet-file>
```

You can also run `parqeye` using `uvx`:

```
uvx parqeye <path-to-parquet-file>
```

# Installation

## Cargo

If you use Rust, build directly from [crates.io](https://crates.io/crates/parqeye)

```
cargo install parqeye
```

## Homebrew

If you have Homebrew, you can install using:

```sh
brew install parqeye
```

## Windows

`parqeye` is available on [winget](https://learn.microsoft.com/en-gb/windows/package-manager/) for Windows. You can install using:

```powershell
winget install --id kaushiksrini.parqeye
```

## Build from Source

You can build from source by downloading the repository and running the following command:

```
cargo build --release
```

## Direct Download

You can download the latest release from the [Releases](https://github.com/kaushiksrini/parqeye/releases) page.

# License

This package is released under the [MIT License](./LICENSE).

# Acknowledgements

- [csvlens](https://github.com/YS-L/csvlens) for the inspiration


# TODOs

- [ ] Lazy/streaming loading of parquet files.
- [ ] Filter columns by value in the visualize tab.
- [ ] Read parquet files on the cloud (`s3://...`).
