import { mount, unmount } from 'svelte';
import ArtworkPicker from './ArtworkPicker.svelte';

/** @param {import('./artwork').ArtworkPickerOptions} options */
export function openArtworkPicker(options) {
  if (!options.images.length) return Promise.resolve(null);
  const returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const target = document.createElement('div');
  document.body.append(target);
  return new Promise((resolve) => {
    const component = mount(ArtworkPicker, {
      target,
      props: {
        ...options,
        returnFocus,
        onfinish: async (choice) => {
          await unmount(component);
          target.remove();
          resolve(choice);
        },
      },
    });
  });
}
