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
