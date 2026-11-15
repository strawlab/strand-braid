# pending-file

Write a file beside its destination and move it into place only once it is
complete.

This is for *derived* outputs: the results of analysis and conversion programs,
which can be made again from their inputs. Such a program should never leave a
file at its destination that merely looks finished, whether it fails, is
interrupted with Ctrl-C, or is killed.

Original data is another matter. A program that saves what cannot be recorded
again must not delete what it has when it fails. It uses
`PendingFile::create_original`: the data is then written as `<name>.partial`
just the same, and left there to be rescued if the program does not get to the
end. Either way, a file at the destination's own name is always a finished one.

```rust
use std::io::Write;

use pending_file::PendingFile;

fn save(dest: &std::path::Path) -> std::io::Result<()> {
    // `file` is an ordinary `std::fs::File`, open on `<dest>.partial`.
    let (mut file, pending) = PendingFile::create(dest)?;
    file.write_all(b"results")?;
    // Until here, an early return, a panic or a kill leaves `dest` untouched.
    pending.commit()
}
```

- The data is written to `<name>.partial` in the destination's own directory,
  so the final step is a rename within one filesystem.
- `commit()` moves it to the destination. `PendingFile::create` replaces an
  existing destination at that moment, and not before, so a failed run does not
  cost the result of an earlier one. `PendingFile::create_new` refuses to
  replace one, both up front and at `commit()`.
- Dropping the `PendingFile` without `commit()` deletes the partial file. One
  made with `create_original` is left instead, and `create_original` refuses to
  start over a partial file which is already there.
- A process that is killed cannot run that cleanup. It leaves the `.partial`
  file behind, where its name says what it is, and the destination is still
  untouched. The next run to the same destination replaces it.
- The partial file is locked while it is open, so a second program writing to
  the same destination at the same time fails rather than corrupting the first.
  Where the filesystem cannot lock, that goes undetected.

Nothing is synced to disk before the rename. After a power failure the
destination may therefore hold an incomplete file; this crate guards against
failed and interrupted programs, not against that.
