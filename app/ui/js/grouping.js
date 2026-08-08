// How the file table buckets rows (#143 split it out of app.js).
//
// Grouping is a view overlay: it decides which bucket a track falls in and what
// that bucket is called, and never touches the order of `tracks` itself. Kept
// apart from the table renderer because GENERATOR reads the same keys when it
// numbers tracks per group.
import { fileName } from "./dom.js";
import { DROP_LOOSE_KEY } from "./fields.js";
import { columnLabel } from "./columns.js";
import { dropFolders, groupBy, sessionRoot } from "./state.js";

// The grouping-key value for a track under the active `groupBy`.
function groupKeyOf(track) {
  switch (groupBy) {
    case "drop":
      // A file-set drop (#127): bucket under the longest dropped folder that is
      // an ancestor of the file; loose files fall through to the Files bucket.
      return dropGroupKey(track.path);
    case "folder": {
      const i = Math.max(track.path.lastIndexOf("/"), track.path.lastIndexOf("\\"));
      return i >= 0 ? track.path.slice(0, i) : "";
    }
    case "artist":
      return track.tags.artist || "";
    case "album":
      return track.tags.album || "";
    case "release":
      // Whichever provider id was stored on import (#20). MusicBrainz first;
      // ids don't collide across providers (UUID vs integer).
      return (
        track.tags["custom:MUSICBRAINZ_ALBUMID"] ||
        track.tags["custom:DISCOGS_RELEASE_ID"] ||
        ""
      );
    // Any modeled tag field (#43): group by its value (artist, album, year,
    // composer, …), the same way the built-in groupings work.
    default:
      return track.tags[groupBy] || "";
  }
}

// The dropped folder a file belongs to (longest ancestor wins so nested dropped
// folders bucket correctly), or DROP_LOOSE_KEY when it's a loose dropped file.
function dropGroupKey(path) {
  let best = null;
  for (const folder of dropFolders || []) {
    if ((path.startsWith(folder + "/") || path.startsWith(folder + "\\")) &&
        (best === null || folder.length > best.length)) {
      best = folder;
    }
  }
  return best === null ? DROP_LOOSE_KEY : best;
}

// A folder-group header (#129): the group directory's path relative to the
// session root, starting with the root's own name (e.g. "Album/CD1"), so nested
// folders read as a tree rather than a bare leaf. Falls back to the leaf name
// for a folder outside the root (shouldn't happen in a normal session).
function folderGroupLabel(key) {
  const root = (sessionRoot || "").replace(/[\\/]+$/, "");
  const rootLeaf = fileName(root);
  if (root && key === root) return rootLeaf;
  if (root && (key.startsWith(root + "/") || key.startsWith(root + "\\"))) {
    const rel = key.slice(root.length).replace(/^[\\/]+/, "").replace(/\\/g, "/");
    return `${rootLeaf}/${rel}`;
  }
  return fileName(key);
}

// Human label for a group header ("(no artist)" etc.; folder shows its path).
function groupLabel(key) {
  if (groupBy === "drop") {
    return key === DROP_LOOSE_KEY ? "Files" : fileName(key);
  }
  if (key === "") {
    if (groupBy === "folder") return "(no folder)";
    if (groupBy === "release") return "(no release id)";
    return `(no ${columnLabel(groupBy).toLowerCase()})`;
  }
  if (groupBy === "folder") return folderGroupLabel(key);
  // Release ids (esp. MusicBrainz UUIDs) are long; show a short, stable prefix.
  if (groupBy === "release") {
    return key.length > 12 ? `Release ${key.slice(0, 8)}…` : `Release ${key}`;
  }
  return key;
}

export { groupKeyOf, dropGroupKey, folderGroupLabel, groupLabel };
