import { test } from 'node:test';
import assert from 'node:assert/strict';
import { describeEarthMessage, policyChanges, itemFromLink, itemFromEvent } from '../src/feed.js';

const policy = (v, levels, extra = {}) => ({
  policy_version: v,
  default_level: 2,
  levels,
  level3: { max_wait_s: 3600, default_fallback: 'abort' },
  ...extra,
});

test('policy updates list the levels they change', () => {
  const before = policy(1, { tow: 2, replan: 2, dig: 1, cannibalize: 3 });
  const after = policy(2, { tow: 1, replan: 1, dig: 1, cannibalize: 4, service: 1 }, { level3: { max_wait_s: 7200, default_fallback: 'proceed' } });
  assert.deepEqual(policyChanges(before, after), ['cannibalize 3→4', 'replan 2→1', 'service 2→1', 'tow 2→1', 'level-3 wait 1h 00m→2h 00m', 'level-3 fallback abort→proceed']);
  const msg = { type: 'policy_update', message_id: 'e-2', policy: after };
  assert.equal(describeEarthMessage(msg, before), 'policy update v2: cannibalize 3→4, replan 2→1, service 2→1, tow 2→1, level-3 wait 1h 00m→2h 00m, level-3 fallback abort→proceed');
  assert.equal(describeEarthMessage({ ...msg, policy: before }, before), 'policy update v1: no level changes');
  assert.equal(describeEarthMessage(msg), 'policy update v2');
});

test('notifies and approval requests do not repeat their action', () => {
  assert.equal(describeEarthMessage({ type: 'notify', action: 'tow', summary: 'tow: os-001813' }), 'notify (tow): os-001813');
  assert.equal(
    describeEarthMessage({ type: 'notify', action: 'reassign_task', summary: 'reassign_task: sinter (37,39) released by cp-1' }),
    'notify (reassign task): sinter (37,39) released by cp-1',
  );
  assert.equal(describeEarthMessage({ type: 'approval_request', action: 'cannibalize', summary: 'Strip cp-1 for parts' }), 'approval request (cannibalize): Strip cp-1 for parts');
  assert.equal(describeEarthMessage({ type: 'status_report', summary: 'sol 0.50: pad 200/400' }), 'status report: sol 0.50: pad 200/400');
});

test('Earth link items say when messages arrive, or that they never do', () => {
  const sent = itemFromLink('down', { t: 100, deliver: 1540, message: { type: 'status_report', message_id: 'm', summary: 's' } });
  assert.equal(sent.note, 'arrives in 24m 00s');
  assert.equal(sent.cat, 'earth');
  const lost = itemFromLink('up', { t: 100, deliver_t: undefined, message: { type: 'approve', message_id: 'e', request_id: 'r' } });
  assert.equal(lost.note, 'never delivered');
  const arrived = itemFromEvent({ t: 7650, type: 'earth_message', sent_t: 6920, message: { type: 'approve', request_id: 'a-1' } }, 7650);
  assert.equal(arrived.text, 'received from Earth: approve a-1');
  assert.equal(arrived.note, 'sent 12m 10s earlier');
  assert.equal(itemFromEvent({ t: 5, type: 'command_accepted', command_id: 'c', robot_id: 'r', estimated_duration_s: 3 }, 9), null, 'acceptances stay out of the feed');
});
