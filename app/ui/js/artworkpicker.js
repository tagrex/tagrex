// Load the Svelte island on demand; normal editor startup stays independent.
export async function chooseArtwork(options) {
  if (!options.images.length) return null;
  const { openArtworkPicker } = await import('../components/openArtworkPicker.js');
  return openArtworkPicker(options);
}
