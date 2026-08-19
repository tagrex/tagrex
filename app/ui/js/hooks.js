// The seam between the extracted panels and the table core (#143).
//
// A panel that stages an edit has to tell the rest of the UI to catch up —
// repaint the table, re-read the cover well, re-run the preview. Those
// functions still live in app.js, and app.js imports the panels, so a panel
// importing them back would be a cycle.
//
// The list is deliberately short and one-directional — panels reach the table,
// never the other way — and it is the only place where that direction is
// inverted. If the table itself ever moves into its own module the panels will
// import these directly and this file goes away; until then, anything that
// isn't "tell the table to catch up" belongs in a real module, not here.
export const hooks = {
  renderTracks: () => {},
  renderPreview: () => {},
  previewEdits: async () => {},
  refreshCoverWell: () => {},
  openDrop: async () => {},
  updateSortIndicators: () => {},
  // Lend the column measurer real rows for values the window doesn't hold
  // (#189); returns the function that takes them away again.
  mountMeasureRows: () => () => {},
  // Select one file in the table and bring it on screen; false when the open
  // library does not hold it (#216). The player's title uses it to answer
  // "which row is this?".
  revealPath: () => false,
  // Rebuild the table header. Locking a field puts a padlock on its column
  // (#48), and the header is built by columns.js — which already reads the lock
  // set, so locks.js reaching back for it directly would be a cycle.
  renderTableHead: () => {},
  // The files the table shows, in visual order, skipping the ones a collapsed
  // folder hides — what the rows themselves answered before the table was
  // windowed (#189).
  navigablePaths: () => [],
};
