OVERVIEW
--------

We're building a generalized intent framework.


STATUS
------

build tool to publish to optimism

ALSO

advance the design to be read/write in the same UX

WIP
---
- [ ] integrate w/ Alloy APIs for interaction w/ Optimism

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
