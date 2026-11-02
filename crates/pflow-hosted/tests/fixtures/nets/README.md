# Net fixtures

Byte-identical copies from pflow-xyz at `25647767ce7052606bc274f42b42604686a3b15c`
(the net fixtures are unchanged at `origin/main` `5b91835`):

| File | Source |
|------|--------|
| `coffee-shop.jsonld`, `knapsack.jsonld`, `net-a.jsonld`, `net-b.jsonld`, `with-parents.jsonld`, `with-rates.jsonld` | `parity/fixtures/` |
| `golden.json` | `parity/golden.json` (the cross-language CID contract for the six above) |
| `colored.jsonld`, `explode.jsonld`, `gated.jsonld`, `inert.jsonld`, `sir.jsonld`, `tie.jsonld`, `game-nim.jsonld`, `game-colored-nim.jsonld`, `game-bucket.jsonld`, `bundle-solo.json`, `bundle-bad-subnets.json` | `parity/native/fixtures/` |

A mismatch against them is a bug in the port; never edit them here. sha256:

```
17cb889841a6893e48e38acd97badf34c0cb666e062b179f5ea23cb5647205a2  bundle-bad-subnets.json
d6f8757e38a397f359157f9429db8b823c1b38f757a79c3c26719c3bbb5a6a00  bundle-solo.json
b28adc89256254991ade9d746ebc2266e0d804faf84f18fed6aeda92f64349e1  coffee-shop.jsonld
5cd4e105a99a1c7a384388dd47869cef1e029aede735cf56610356c0d00b4a86  colored.jsonld
408da6dd4aff510a8b40681e2c5f1734fba3f1f134ae6b070ccf7fb93417bc6c  explode.jsonld
7b527a11aeced02d483c6e0e105a47ccd574837ba5873fea5a3d20d2fccb7465  game-bucket.jsonld
53c1b1c90e4252efcfca67ed35da83df8cd35f10688d98b629dfc4850602d454  game-colored-nim.jsonld
f268f95e4ed8b6558c1c3c61d42264f39458550ae2080cf63662c7846b33aab3  game-nim.jsonld
29e85d111181897ce76c2c22fc9fde768318cdd2db552d3bc96a02c6cfbaf426  gated.jsonld
c8bf7bedc1643732aa1d16608464e42605856a4f8907247f4e86aa4c49f00a73  golden.json
f64bf88a6b2bf8922f5ebbf81de7db4b82577e93942ac5f5efef7045c56743b0  inert.jsonld
7ceb53638f892c798febfd00faeef0fd9e64713618a75f852aa99ba3871509c3  knapsack.jsonld
f3b7cc20962d9708179990ab503610bb7b6bcfc6708738a91676d71b56b703fb  net-a.jsonld
1acb58b38f1026d7e803d51ca3bceb0b4ccf1016d164bdb10b2c23f54ed5b424  net-b.jsonld
f43a06f99555da97e7d932edee84fe9b0009a5179c8813f812781907ae9bbe56  sir.jsonld
a03acca0894fabcb7098721cdc326514868bab13beb71edac39d7265b9de4c14  tie.jsonld
ce75b8cd51e8ce4f13653ec9a3467b4f7fd857fef0286cfbbec3fb5fa884d767  with-parents.jsonld
b6d12525d1b443272304276d292abc8fd8d571d58bea9b7bd96628ae63c0a09f  with-rates.jsonld
```

`local/` holds small hand-built documents, not from pflow-xyz, for cases the
fixtures above do not reach: `edge.json` (wrong-typed fields go-pflow's reader
skips, a zero weight, initial over capacity, a negative marking, an inhibitor
and a read arc, a dangling arc), `pump.json` (an unbounded net), `flat.json`
(the flat net `pflow_bundle_flatten` returns, which the net tools refuse),
`plan.json` (a `SearchPlan`) and `bare.json` (a composition that is a bare
wrapper around one net).
