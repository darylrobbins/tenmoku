---
task_uid: "01m6pp0jcahrat7byzphv22cn6"
task: "T455"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T282

Findings of the review of T282 that don't block the merge (below merge.block_severity):

- F2 medium kiln/crates/kiln-plugin/src/secrets.rs:227: Durability remains unsafe after a synchronization failure. If rename succeeds but syncing the directory fails, set returns an error while the new name remains visible in the index. Retrying set finds that name and skips write_index, allowing the keychain write without establishing index durability. Similarly, if creating a directory succeeds but syncing its parent fails, the next call's is_dir shortcut skips that synchronization. A subsequent crash can therefore leave a stored secret without its index.
  fix: Establish index and directory ancestry durability before every store set, including retries that find an existing name or directory. Do not treat filesystem visibility as proof of successful synchronization. Add fault-injection tests for failures after rename and directory creation followed by retries.
  instances: kiln/crates/kiln-plugin/src/secrets.rs:116, kiln/crates/kiln-plugin/src/secrets.rs:227, kiln/crates/kiln-plugin/src/secrets.rs:247, kiln/crates/kiln-plugin/src/secrets.rs:258
