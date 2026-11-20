import { test } from 'node:test';
import assert from 'node:assert/strict';

// vinyl.js has no imports, so it loads as it is.
const { mediaKind } = await import('../../app/ui/js/vinyl.js');

test('an EP on CD or as files is not a record (#803)', () => {
  assert.equal(mediaKind('CD, EP'), 'cd');
  assert.equal(mediaKind('File, FLAC, EP'), 'digital');
  assert.equal(mediaKind('File, MP3, 320 kbps, EP'), 'digital');
});

test('EP and LP count only as whole words (#803)', () => {
  assert.equal(mediaKind('CD, Album, Reissue, Repress'), 'cd');
  assert.equal(mediaKind('EP'), 'vinyl');
  assert.equal(mediaKind('LP'), 'vinyl');
});

test('a mixtape on CD or as files is not a cassette (#804)', () => {
  assert.equal(mediaKind('CD, Mixtape'), 'cd');
  assert.equal(mediaKind('File, MP3, Mixtape'), 'digital');
  assert.equal(mediaKind('Cassette, Mixtape'), 'cassette');
  assert.equal(mediaKind('Microcassette'), 'cassette');
  assert.equal(mediaKind('Tape'), 'cassette');
});

test('the usual provider formats keep their medium', () => {
  assert.equal(mediaKind('Vinyl, 12", 33 ⅓ RPM, EP'), 'vinyl');
  assert.equal(mediaKind('Vinyl, LP, Album'), 'vinyl');
  assert.equal(mediaKind('12" Vinyl'), 'vinyl');
  assert.equal(mediaKind('Shellac, 10"'), 'vinyl');
  assert.equal(mediaKind('Cassette, Album'), 'cassette');
  assert.equal(mediaKind('CD, Album'), 'cd');
  assert.equal(mediaKind('CDr'), 'cd');
  assert.equal(mediaKind('CD, DVD'), 'cd');
  assert.equal(mediaKind('Digital Media'), 'digital');
  assert.equal(mediaKind('Digital'), 'digital');
  assert.equal(mediaKind('File'), 'digital');
  assert.equal(mediaKind('DVD, DVD-Video'), 'generic');
  assert.equal(mediaKind(''), 'generic');
  assert.equal(mediaKind(null), 'generic');
});
