import { test } from 'node:test';
import assert from 'node:assert/strict';

// vinyl.js has no imports, so it loads as it is.
const { vinylPositionOf } = await import('../../app/ui/js/vinyl.js');

const track = (tags) => ({ tags });

test('the stored position wins over disc + track (#796)', () => {
  // Numbered by record (#795): B1 is track 3 on disc 1.
  const b1 = track({ media: 'Vinyl', disc: '1', track: '3', 'custom:POSITION': 'B1' });
  assert.equal(vinylPositionOf(b1), 'B1');
  assert.equal(vinylPositionOf(track({ media: 'Vinyl', 'custom:POSITION': 'AA' })), 'AA');
  assert.equal(vinylPositionOf(track({ media: 'Vinyl', 'custom:POSITION': '1A' })), '1A');
});

test('without a side position the column is rebuilt from disc + track', () => {
  assert.equal(vinylPositionOf(track({ media: 'Vinyl', disc: '2', track: '3' })), 'B3');
  // A CD position such as 1-05 is not a side; the track number stands.
  const cd = track({ media: 'CD', disc: '1', track: '5', 'custom:POSITION': '1-05' });
  assert.equal(vinylPositionOf(cd), '5');
});

test('a staged POSITION edit is what the column shows', () => {
  const file = track({ media: 'Vinyl', disc: '1', track: '1', 'custom:POSITION': 'A1' });
  assert.equal(vinylPositionOf(file, new Map([['custom:POSITION', 'B2']])), 'B2');
});
