// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

import {Script, console} from "forge-std/Script.sol";
import {MyStateMachine} from "../src/MyStateMachine.sol";

contract MyStateMachineScript is Script {
    MyStateMachine public myStateMachine;

    function setUp() public {}

    function run() public {
        vm.startBroadcast();

        myStateMachine = new MyStateMachine();

        vm.stopBroadcast();
    }
}
