// The seam between the extracted panels and the table core (#143).
//
// A panel that stages an edit has to tell the rest of the UI to catch up —
// repaint the table, re-read the cover well, re-run the preview. Those
// functions still live in app.js, and app.js imports the panels, so a panel
// importing them back would be a cycle.
//
// This is deliberately a short list, and it shrinks: as the table itself moves
// into its own module the panels will import what they need directly, and each
// entry here goes away. Anything longer-lived than that belongs in a real
// module, not here.
export const hooks = {
  renderTracks: () => {},
  previewEdits: async () => {},
  refreshCoverWell: () => {},
};
