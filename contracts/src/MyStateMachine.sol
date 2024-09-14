// SPDX-License-Identifier: MIT
pragma solidity >=0.8.0;

library Model {

    event SignaledEvent(
        uint8 indexed role,
        uint8 indexed actionId,
        uint256 indexed scalar,
        uint256 sequence
    );

    struct PetriNet {
        Place[] places;
        Transition[] transitions;
    }

    struct Position {
        uint8 x;
        uint8 y;
    }

    struct Transition {
        string label;
        uint8 offset;
        Position position;
        uint8 role;
        int256[] delta;
        int256[] guard;
    }

    struct Place {
        string label;
        uint8 offset;
        Position position;
        uint256 initial;
        uint256 capacity;
    }

    struct Head {
        uint256[10] latestBlocks;
        uint256 sequence;
        int256[] state;
        Place[] places;
        Transition[] transitions;
    }

}

interface ModelInterface {
    function context() external returns (Model.Head memory);

    function signal(uint8 action, uint256 scalar) external;

    function signalMany(uint8[] calldata actions, uint256[] calldata scalars) external;
}

abstract contract PflowDSL {
    Model.Place[] internal places;
    Model.Transition[] internal transitions;

    function cell(string memory label, uint256 initial, uint256 capacity, Model.Position memory position) internal returns (Model.Place memory) {
        Model.Place memory p = Model.Place(label, uint8(places.length), position, initial, capacity);
        places.push(p);
        return p;
    }

    function func(string memory label, uint8 vectorSize, uint8 action, uint8 role, Model.Position memory position) internal returns (Model.Transition memory) {
        require(uint8(transitions.length) == action, "transaction => enum mismatch");
        Model.Transition memory t = Model.Transition(label, action, position, role, new int256[](vectorSize), new int256[](vectorSize));
        transitions.push(t);
        return t;
    }

    function arrow(int256 weight, Model.Place memory p, Model.Transition memory t) internal {
        require(weight > 0, "weight must be > 0");
        transitions[t.offset].delta[p.offset] = 0 - weight;
    }

    function arrow(int256 weight, Model.Transition memory t, Model.Place memory p) internal {
        require(weight > 0, "weight must be > 0");
        transitions[t.offset].delta[p.offset] = weight;
    }

    // inhibit transition after threshold weight is reached
    function guard(int256 weight, Model.Place memory p, Model.Transition memory t) internal {
        require(weight > 0, "weight must be > 0");
        transitions[t.offset].guard[p.offset] = 0 - weight;
    }

    // inhibit transition until threshold weight is reached
    function guard(int256 weight, Model.Transition memory t, Model.Place memory p) internal {
        require(weight > 0, "weight must be > 0");
        transitions[t.offset].guard[p.offset] = weight;
    }
}

abstract contract Metamodel is PflowDSL, ModelInterface {

    // sequence is a monotonically increasing counter for each signal
    uint256 public sequence = 0;

    // latestBlocks stores the last 10 block numbers when a signal was received
    uint256[10] public latestBlocks = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    // transform is a hook for derived contracts to implement state transitions
    function transform(uint8 i, Model.Transition memory t, uint256 scalar) internal virtual;

    // isInhibited is a hook for derived contracts to implement transition guards
    function isInhibited(Model.Transition memory t) internal view virtual returns (bool);

    // hasPermission implements an ACL for transitions based on user roles
    function hasPermission(Model.Transition memory t) internal view virtual returns (bool);

    // context returns the current state of the model
    function context() external view virtual returns (Model.Head memory);

    // signal is the main entry point for signaling transitions
    function _signal(uint8 action, uint256 scalar) internal {
        Model.Transition memory t = transitions[action];
        require(!isInhibited(t), "inhibited");
        assert(action == t.offset);
        for (uint8 i = 0; i < uint8(places.length); i++) {
            transform(i, t, scalar);
        }
        sequence++;
        emit Model.SignaledEvent(t.role, action, scalar, sequence);
    }

    function updateBlocks(uint256 blockNumber) internal {
        for (uint8 i = 0; i < 9; i++) {
            latestBlocks[i] = latestBlocks[i + 1];
        }
        latestBlocks[9] = blockNumber;
    }

    function signal(uint8 action, uint256 scalar) external {
        _signal(action, scalar);
        updateBlocks(block.number);
    }

    function signalMany(uint8[] calldata actions, uint256[] calldata scalars) external {
        require(actions.length == scalars.length, "ModelRegistry: invalid input");
        for (uint256 i = 0; i < actions.length; i++) {
            _signal(actions[i], scalars[i]);
        }
        updateBlocks(block.number);
    }

}


abstract contract MyModelContract is Metamodel {
    enum Roles {role0, HALT}
    enum Properties {place0, SIZE}
    enum Actions {txn0, txn1, txn2, txn3, HALT}

    int256[] public state = new int256[](uint8(Properties.SIZE));

    constructor() {
        cell("place0", 0, 3, Model.Position(1, 2));


        func("txn0", uint8(Properties.SIZE), uint8(0), uint8(Roles.role0), Model.Position(0, 1));
        func("txn1", uint8(Properties.SIZE), uint8(1), uint8(Roles.role0), Model.Position(2, 1));
        func("txn2", uint8(Properties.SIZE), uint8(2), uint8(Roles.role0), Model.Position(0, 3));
        func("txn3", uint8(Properties.SIZE), uint8(3), uint8(Roles.role0), Model.Position(2, 3));


        arrow(1, transitions[0], places[0]);
        arrow(3, places[0], transitions[1]);
        guard(3, transitions[2], places[0]);
        guard(1, places[0], transitions[3]);


        for (uint8 i = 0; i < uint8(Properties.SIZE); i++) {
            state[i] = int256(places[i].initial);
        }
    }
}

contract MyStateMachine is MyModelContract {

    function isInhibited(Model.Transition memory t) internal view override returns (bool) {
        for (uint8 i = 0; i < uint8(Properties.SIZE); i++) {
            if (t.guard[i] != 0) {
                if (t.guard[i] < 0) {
                    // inhibit unless condition is met
                    if ((state[i] + t.guard[i]) >= 0) {
                        return true;
                    }
                } else {
                    // inhibit until condition is met
                    if ((state[i] - t.guard[i]) < 0) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    function hasPermission(Model.Transition memory t) internal view override returns (bool) {
        return t.role < uint8(Roles.HALT);
    }

    function transform(uint8 i, Model.Transition memory t, uint256 scalar) internal override {
        require(scalar > 0, "invalid scalar");
        if (t.delta[i] != 0) {
            state[i] = state[i] + t.delta[i] * int256(scalar);
            require(state[i] >= 0, "underflow");
            if (places[i].capacity > 0) {
                require(state[i] <= int256(places[i].capacity), "overflow");
            }
        }
    }

    function context() external view override returns (Model.Head memory) {
        return Model.Head(latestBlocks, sequence, state, places, transitions);
    }

}
