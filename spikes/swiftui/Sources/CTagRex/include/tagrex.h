// The read-only C ABI exposed by crates/ffi (#271).
#ifndef TAGREX_H
#define TAGREX_H

/// Scan a folder and return the library as JSON. The caller owns the string.
char *tagrex_scan_json(const char *root);

/// Give back a string returned by tagrex_scan_json.
void tagrex_string_free(char *ptr);

#endif
