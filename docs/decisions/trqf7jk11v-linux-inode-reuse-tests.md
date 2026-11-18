# T693: tests that swap an entry must not let the new one reuse the inode

Entries are identified by (device, inode). Both tests removed a file and wrote
a new one at the same path. On ext4 the new file gets the freed inode back, so
the swap looks like the same entry; macOS and tmpfs don't reuse it at once.

Seen before the fix, on ext4 (dev 2307, TMPDIR on the repo disk; both tests failed):

- fe-project `another_entry_in_its_place_in_the_trash_is_not_taken`:
  before (2307, 7340788), after (2307, 7340788). `restore` returned Ok.
- fe-workbench `partial_failure_names_what_failed`:
  before 7340792, after 7340792. The last toast stayed "2 items couldn't be moved".

On helium's tmpfs /tmp (dev 36) the inodes differed (e.g. 106307023 -> 106307026)
and the tests passed, which is why a default TMPDIR hid it.

Fix (tests only): hard-link the old file aside before removing it, so its inode
stays in use while the new file is written. No production change. This is
platform-neutral, so the macOS behaviour is unchanged.

After the fix: 20 of 20 runs of each test pass on ext4.
