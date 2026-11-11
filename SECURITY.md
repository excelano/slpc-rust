# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately, using the **Report a vulnerability** button on the Security tab of the repository concerned. If that button is not there, or you would rather not go through GitHub, email david.anderson@excelano.com instead. I aim to respond within seven days.

Please do not open public issues for security problems.

## Supported versions

The latest release receives security fixes. Older versions are not supported. A fix ships in a new release; update the way you installed.

## What slipcase can access

slipcase is a CLI that runs locally on your machine, and `slpc` is the library behind it. Both read the file you point them at and write only where you tell them to. Neither makes a network call of any kind, has an auth layer, or implements any administrative operation. They can read and write only what your operating-system user already can.

A few things are worth stating because they are the ones a container format could plausibly get wrong.

**Unpacking writes the content file, and other members only when asked.** `content.file` is a plain filename and never a path, which the specification states as a list of exclusions in its section 2.3 and this implementation checks in full before the name is used. A name breaking any of them is rejected rather than repaired, so a content file cannot be written outside the destination directory. The content member must also be a regular file entry, so a symbolic link, a directory, or a device cannot be written in a content file's place. Other members reach disk only on request: `--flyleaf` adds `slipcase.flyleaf.toml`, `--member NAME` adds the additional member of that name, and `--all-members` adds every one. An additional member's name may carry directories, so each `/`-separated segment is checked instead: none may be empty, `.` or `..`, and the name may hold no backslash, colon, or control character. Everything asked for is checked before anything is written, no symbolic link beneath the destination is followed, nothing is replaced without `--force`, and if writing fails partway the files and directories the request created are removed. Checking a directory and then writing into it is not atomic, so a process racing to swap a link into the destination is not stopped: extract into a directory no other user can write to.

**Staying inside the directory is not the same as being a file.** Because the checked name contains no separator, joining it to a directory cannot leave that directory, which is true and does not settle the question. Windows resolves a handful of names to devices wherever they appear, so `CON` does not leave the destination directory; it is simply not in it, and writing there would discard the content file while reading it back could hang. The destination is therefore addressed in a form those names are not resolved in, so a conformant container naming its content file `CON` produces an ordinary file.

**Rewriting a container keeps where it came from.** `repack` replaces a container by writing a new file and renaming it over the old one, and a new file carries no mark of its own, so without care an edit to the flyleaf of a downloaded container would strip the record of where it came from, and every content file unpacked from it afterwards would be unmarked too. Replacing a container in place carries the mark onto the replacement, and `repack -o` carries it onto the new file.

**Unpacking carries where the container came from onto the content file.** If the container is marked as downloaded — `com.apple.quarantine`, a `Zone.Identifier` stream, `user.xdg.origin.url` — the content file is marked the same way, so that whatever opens it next sees a file that arrived from elsewhere rather than one this machine made. Where the platform gates opening on that mark and it cannot be written, `unpack` removes the content file and says so rather than leaving one that opens without the warning its origin earned. Apart from removing what a failed request created, this is the one case in which unpacking deletes something it wrote. A container read from standard input has no source to read a mark from and is unpacked without one.

**A content file is never executed, opened, or handed to another program.** The tool has no verb that runs anything. Nothing here inspects a content file's type or acts on it, and a container that names its content file `report.pdf` gets no different treatment from one that names it `setup.exe`.

**Identifying a container is bounded.** Resource limits on the content file belong to `zip` and `toml_edit` and apply equally to every other consumer of those crates, because nothing inflates it until you ask. The flyleaf member is different: deciding whether a file is a container means decompressing that member and parsing it as TOML, so the memory is spent before anything about the file is known, which is not the position of a general ZIP consumer who chooses what to extract. A small container whose flyleaf member deflates at a high ratio can cost hundreds of megabytes to identify. The flyleaf member is therefore bounded, 1 MiB by default and adjustable through `Limits`, and a container over the bound is reported as undetermined rather than as non-conformant, because the bound belongs to the reader and not to the file.

**A content name is escaped before it is shown to you.** The specification permits the Unicode bidirectional formatting characters in `content.file`, because they are legal filenames everywhere and excluding them would make the name rules a table of special cases. A content file called `report<U+202E>fdp.exe` therefore reads as `report.pdf` in any terminal that applies the override. `slipcase validate` escapes it, and `slipcase info` escapes it when it is writing to a terminal — when it is redirected into a file or a pipe it still reproduces the flyleaf member byte for byte, since that is what a caller redirecting it asked for.

Beyond those, a container is an ordinary ZIP archive, and the usual cautions about archives from untrusted sources apply to it exactly as they do to any other.

## What slipcase stores

Nothing beyond the files you ask it to write. There is no configuration directory, no history file, no cache, no telemetry, no analytics, and no remote logging. Reading from standard input spools to a temporary file that is unlinked as it is created, so it leaves nothing behind however the process ends.

## Verifying releases

Every GitHub release includes a `.sha256` file next to each archive listing its SHA-256 hash. Verify any download before running it:

    sha256sum slipcase-x86_64-unknown-linux-gnu.tar.xz
    # compare against the value in slipcase-x86_64-unknown-linux-gnu.tar.xz.sha256

Release artifacts are built by `build-release`, the release program in excelano/shipping, from a tagged commit using the cargo-dist configuration in this repository (`dist-workspace.toml`). That build configuration is public and auditable.
