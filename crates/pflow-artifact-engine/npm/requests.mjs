// The calls each golden records for a net: analyze at the default and a small
// state budget, then fire every transition enabled at the initial marking, and
// one transition that is not (or a name that does not exist) to pin the refusal.
export function requests(net) {
  const ts = Array.isArray(net.transitions)
    ? net.transitions.map((t) => t.id ?? t.name)
    : Object.keys(net.transitions ?? {});
  const calls = [
    ['analyze', { net }],
    ['analyze', { net, max_states: 5 }],
    ['fire', { net }],
  ];
  for (const t of ts) calls.push(['fire', { net, transition: t }]);
  calls.push(['fire', { net, transition: '__no_such_transition__' }]);
  return calls;
}

// The deliberate departures from the recorded inline engine, applied to its
// replies by gen-goldens.mjs and listed in each golden's "amendments".
//
// 1. (2026-11-10, @pflow-xyz/engine 0.2.0) "bounded" is three-valued: a search
//    that stopped at max_states before any marking passed the token limit has
//    not proved the net bounded, so the answer is "unknown" (as deadlock_free,
//    live and quasi_live already were), not true. pflow-reachability's
//    AnalysisResult::bounded_verdict.
export const BOUNDED_WHY = 'a search stopped at max_states (complete: false) has not proved boundedness';
export function amend(calls) {
  const amendments = [];
  calls.forEach((c, i) => {
    const r = c.reply;
    if (c.fn === 'analyze' && r && r.complete === false && r.bounded === true) {
      r.bounded = 'unknown';
      amendments.push({ call: i, field: 'bounded', was: true, now: 'unknown', why: BOUNDED_WHY });
    }
  });
  return amendments;
}
