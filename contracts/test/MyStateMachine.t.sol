// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

import {Test, console} from "forge-std/Test.sol";
import {MyStateMachine} from "../src/MyStateMachine.sol";

contract MyStateMachineTest is Test {
    MyStateMachine public sm;

    function setUp() public {
        sm = new MyStateMachine();
    }

    function test_Increment() public {
    }

    // function testFuzz_SetNumber(uint256 x) public {
    //     counter.setNumber(x);
    //     assertEq(counter.number(), x);
    // }
}
