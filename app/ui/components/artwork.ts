export interface CoverArt {
  mime: string;
  data_base64: string;
}
export interface ReleaseImage {
  url: string;
  width?: number;
  height?: number;
}
export interface ArtworkChoice {
  rotation?: number;
  index: number;
  cover: CoverArt;
}
export interface ArtworkPickerOptions {
  title: string;
  links?: ArtworkLink[];
  openLink?: (link: ArtworkLink) => Promise<void>;
  images: ReleaseImage[];
  selected?: number;
  rotations?: Record<number, number>;
  fetchImage: (url: string) => Promise<CoverArt>;
}

export interface ArtworkLink {
  name: string;
  entity: 'release' | 'artist' | 'label';
  id: string;
  url: string;
}

export async function decodeArtwork(cover: CoverArt): Promise<HTMLImageElement> {
  const image = new Image();
  image.src = `data:${cover.mime};base64,${cover.data_base64}`;
  await image.decode();
  return image;
}

/** Rotate the original pixels, independent of preview zoom/pan. `decoded` is
 * the same cover already decoded for display; passing it avoids decoding the
 * artwork twice on the way to Apply. */
export async function rotateArtwork(cover: CoverArt, degrees: number, decoded?: HTMLImageElement | null): Promise<CoverArt> {
  const angle = ((degrees % 360) + 360) % 360;
  if (!angle) return cover;
  const image = decoded || await decodeArtwork(cover);
  const canvas = document.createElement('canvas');
  const swapped = angle === 90 || angle === 270;
  canvas.width = swapped ? image.naturalHeight : image.naturalWidth;
  canvas.height = swapped ? image.naturalWidth : image.naturalHeight;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('Image canvas unavailable');
  context.translate(canvas.width / 2, canvas.height / 2);
  context.rotate(angle * Math.PI / 180);
  context.drawImage(image, -image.naturalWidth / 2, -image.naturalHeight / 2);
  const mime = cover.mime === 'image/jpeg' ? 'image/jpeg' : 'image/png';
  return { ...cover, mime, data_base64: canvas.toDataURL(mime, .95).split(',')[1] };
}
