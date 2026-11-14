import '../../../app/ui/shadcn.css';
import '../../../app/ui/style.css';
import '../../../app/ui/workbench.css';
import { chooseArtwork } from '../../../app/ui/js/artworkpicker.js';
import { initArtworkWheel } from '../../../app/ui/lib/artwork-wheel';

initArtworkWheel(document);

const params = new URLSearchParams(location.search);
const count = Number(params.get('count') || 12);
const images = Array.from({ length: count }, (_, i) => ({ url: `image-${i}`, width: i === 1 ? 900 : 600, height: 600 }));
const fixtures = images.map((_, i) => {
  const canvas = document.createElement('canvas');
  canvas.width = images[i].width;
  canvas.height = 600;
  const ctx = canvas.getContext('2d');
  ctx.fillStyle = ['#b64532', '#324fb6', '#936833'][i % 3];
  ctx.fillRect(0, 0, canvas.width, 600);
  ctx.fillStyle = '#00ff00'; ctx.fillRect(0, 0, 20, 20);
  ctx.fillStyle = '#fff'; ctx.font = '48px sans-serif'; ctx.fillText(`ARTWORK ${i + 1}`, 40, 90);
  return { mime: 'image/png', data_base64: canvas.toDataURL().split(',')[1] };
});
window.artworkTest = { opened: [], calls: [], fail: -1, delays: {}, fixtures };
window.artworkResult = undefined;
document.getElementById('launch').onclick = async () => {
  window.artworkResult = undefined;
  window.artworkResult = await chooseArtwork({
    title: 'Various — Helter Skelter · The Strings of Life', images, selected: 0,
    links: [
      { name: 'Helter Skelter', entity: 'label', id: '456', url: 'https://www.discogs.com/label/456' },
      { name: 'Various', entity: 'artist', id: '194', url: 'https://www.discogs.com/artist/194' },
      { name: 'HS19 12P — The Strings of Life', entity: 'release', id: '123', url: 'https://www.discogs.com/release/123' },
    ],
    openLink: async (link) => { window.artworkTest.opened.push(link); },
    fetchImage: async (url) => {
      const index = Number(url.slice(6));
      window.artworkTest.calls.push(index);
      await new Promise((resolve) => setTimeout(resolve, window.artworkTest.delays[index] || 20));
      if (window.artworkTest.fail === index) throw new Error('Unavailable');
      return fixtures[index];
    },
  });
};
