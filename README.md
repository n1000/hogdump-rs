# hogdump

Descent HOG file extraction / creation utility.

This project was mostly done to continue learning Rust, and may or may not be
maintained and may or may not be useful :)

## Help Overview

```console
HOG File Dump Utility

Usage: hogdump [OPTIONS] <FILE>...

Arguments:
  <FILE>...  The files to operate on (1 or more)

Options:
  -x, --extract          Extract the contents of the provided hog file(s)
  -c, --create <CREATE>  Create hog file out of the provided file(s)
  -o, --overwrite        Overwrite files
  -v, --verbose          Display more information during processing
  -h, --help             Print help information
  -V, --version          Print version information
```

## Examples

### Example - Extract HOG file

```console
$ mkdir tmp && cd tmp
$ hogdump -x ../descent.hog
  ../descent.hog: bitmaps.bin: wrote 41634 bytes
  ../descent.hog: descent.txb: wrote 11187 bytes
  ../descent.hog: briefing.txb: wrote 15491 bytes
  ../descent.hog: credits.txb: wrote 1677 bytes
  ../descent.hog: ending.txb: wrote 720 bytes
...
  ../descent.hog: flare.pof: wrote 486 bytes
  ../descent.hog: smissile.pof: wrote 1580 bytes
Processed 106 files, extracted 106 files (2337968 bytes), skipped 0 files.
```

By default, `hogdump` will refuse to overwrite files, unless the `-o`
(overwrite) option is given. For example, running the exact same command above
a second time:

```console
$ hogdump -x ../descent.hog
  ../descent.hog: bitmaps.bin: skipping (already exists)
  ../descent.hog: descent.txb: skipping (already exists)
  ../descent.hog: briefing.txb: skipping (already exists)
  ../descent.hog: credits.txb: skipping (already exists)
  ../descent.hog: ending.txb: skipping (already exists)
...
  ../descent.hog: flare.pof: skipping (already exists)
  ../descent.hog: smissile.pof: skipping (already exists)
Processed 106 files, extracted 0 files (0 bytes), skipped 106 files.
```

Adding the `-o` option: `hogdump -ox ../descent.hog` will cause the files to be
overwritten.

### Example - Create HOG file

This example creates a new hog file called "new_descent.hog", from the files
extracted in the previous example (in the `tmp` directory).

```console
$ hogdump -c new_descent.hog tmp/*
new_descent.hog: added file "tmp/bitmaps.bin" (41634 bytes).
new_descent.hog: added file "tmp/boss01.pof" (9074 bytes).
new_descent.hog: added file "tmp/brief01.pcx" (30474 bytes).
...
new_descent.hog: added file "tmp/sun.bbm" (9296 bytes).
new_descent.hog: added file "tmp/venus01.pcx" (41861 bytes).
```

HOG files can only support filenames with 13 characters, and only the file
base name is stored in the HOG file (not the complete path). For the example
below the `tmp/` prefix is stripped off of the stored filename. Extracting the
HOG later will not create a `tmp` directory.
