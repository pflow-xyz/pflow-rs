OVERVIEW
--------

This is the local UX for p2p users - should be oriented toward private usage.

For example, whenever we encounter a compatible contract address,
we will index that contract and provide a user interface to interact with it.

TODO: probably keep everything in sqlite!!!
this is an end-user tool, not a developer tool (think courtesy node)

STATUS
------

build tool to publish to optimism

ALSO

advance the design to be read/write in the same UX

WIP
---
- [ ] refer to contracts by address *ONLY* in URLs
- [ ] should we support loading models from contracts (YES!)
- [ ] optimism.pflow.xyz/contract/0x1234

BACKLOG
-------
- [ ] publish models to Optimism (use sponsored funds)

- [ ] provide a developer tool for rust + solidity echo system

- [ ] adjust to host svg images 'live' from the blockchain

- [ ] server-side rendering / indexing of pflow models / designs
    - [ ] add/test SVG route 
    - [ ] add/test Json routes

ICEBOX
------
- [ ] consider higher order design tools that connect and re-mix pflow models

DONE
----
- [x] generate smart contract types from the solidity contract
- [x] inject model into sessionData in dynamic html response
