// The read-only C ABI exposed by crates/ffi (#271).
#ifndef TAGREX_H
#define TAGREX_H

/// Scan a folder and return the library as JSON. The caller owns the string.
char *tagrex_scan_json(const char *root);

/// Write a set of tag edits as one journaled batch. Takes and returns JSON.
char *tagrex_apply_json(const char *request);

/// Take back the most recent batch in the journal. Takes and returns JSON.
char *tagrex_undo_json(const char *request);

/// Give back a string returned by any of the calls above.
void tagrex_string_free(char *ptr);

#endif
