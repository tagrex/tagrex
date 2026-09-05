// The C ABI exposed by crates/ffi (#271, #293): a session over the command
// layer. Open a library, invoke commands by name, close it. Every call that
// returns a char* hands the caller a string it must give back to
// tagrex_string_free.
#ifndef TAGREX_H
#define TAGREX_H

/// An open library session. Opaque: the layout lives in Rust.
typedef struct TagRexSession TagRexSession;

/// Open `root` as a library, storing the handle through `out`. The journal lives
/// at `config_dir/journal.sqlite`. Returns a JSON `{"ok":null}` / `{"error":…}`
/// envelope; on error the handle is left null.
char *tagrex_open(const char *root, const char *config_dir, TagRexSession **out);

/// Open a drag-and-drop of files and/or folders (a JSON array of path strings),
/// storing the handle through `out`. Returns `{"ok":<DropResult>}` / `{"error":…}`.
char *tagrex_open_drop(const char *paths_json, const char *config_dir, TagRexSession **out);

/// Run a command against an open session. `args_json` is a JSON object keyed the
/// way the command names its parameters. Returns `{"ok":<result>}` / `{"error":…}`.
char *tagrex_invoke(TagRexSession *handle, const char *cmd, const char *args_json);

/// Close a session. The handle must not be used afterwards. Null is ignored.
void tagrex_close(TagRexSession *handle);

/// Give back a string returned by any of the calls above.
void tagrex_string_free(char *ptr);

#endif
