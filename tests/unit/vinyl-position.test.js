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

const { layoutVinylPositions, sideOfPosition, vinylNumberingOf } = await import('../../app/ui/js/vinyl.js');

test('sides are read off a position (#797)', () => {
  assert.deepEqual(sideOfPosition('A1'), { side: 'A', digits: '1' });
  assert.deepEqual(sideOfPosition('aa'), { side: 'AA', digits: '' });
  assert.deepEqual(sideOfPosition('12B'), { side: 'B', digits: '12' });
  for (const none of ['5', '1-05', 'AB', 'A1a', '']) assert.equal(sideOfPosition(none), null);
});

test('positions are numbered by record or by side, as the backend does', () => {
  const positions = ['A1', 'A2', 'B1', 'C1', '7', 'D1'];
  const byRecord = layoutVinylPositions(positions, 'records');
  assert.deepEqual(
    byRecord.places.map((place) => place && `${place.disc}/${place.track}`),
    ['1/1', '1/2', '1/3', '2/1', null, '2/2']
  );
  assert.equal(byRecord.discs, 2);
  const bySide = layoutVinylPositions(positions, 'sides');
  assert.deepEqual(
    bySide.places.map((place) => place && `${place.disc}/${place.track}`),
    ['1/1', '1/2', '2/1', '3/1', null, '4/1']
  );
  assert.equal(bySide.discs, 4);
  // A double A-side single is one record.
  assert.equal(layoutVinylPositions(['A', 'AA'], 'records').discs, 1);
});

test('the numbering setting reads old settings files too', () => {
  assert.equal(vinylNumberingOf({}), 'records');
  assert.equal(vinylNumberingOf({ import_vinyl_position: true }), 'position');
  assert.equal(vinylNumberingOf({ import_vinyl_numbering: 'sides', import_vinyl_position: true }), 'sides');
});
