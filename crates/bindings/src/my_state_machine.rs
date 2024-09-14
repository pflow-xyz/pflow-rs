pub use my_state_machine::*;
/// This module was auto-generated with ethers-rs Abigen.
/// More information at: <https://github.com/gakonst/ethers-rs>
#[allow(
    clippy::enum_variant_names,
    clippy::too_many_arguments,
    clippy::upper_case_acronyms,
    clippy::type_complexity,
    dead_code,
    non_camel_case_types,
)]
pub mod my_state_machine {
    pub use super::super::shared_types::*;
    #[allow(deprecated)]
    fn __abi() -> ::ethers::core::abi::Abi {
        ::ethers::core::abi::ethabi::Contract {
            constructor: ::core::option::Option::None,
            functions: ::core::convert::From::from([
                (
                    ::std::borrow::ToOwned::to_owned("context"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("context"),
                            inputs: ::std::vec![],
                            outputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Tuple(
                                        ::std::vec![
                                            ::ethers::core::abi::ethabi::ParamType::FixedArray(
                                                ::std::boxed::Box::new(
                                                    ::ethers::core::abi::ethabi::ParamType::Uint(256usize),
                                                ),
                                                10usize,
                                            ),
                                            ::ethers::core::abi::ethabi::ParamType::Uint(256usize),
                                            ::ethers::core::abi::ethabi::ParamType::Array(
                                                ::std::boxed::Box::new(
                                                    ::ethers::core::abi::ethabi::ParamType::Int(256usize),
                                                ),
                                            ),
                                            ::ethers::core::abi::ethabi::ParamType::Array(
                                                ::std::boxed::Box::new(
                                                    ::ethers::core::abi::ethabi::ParamType::Tuple(
                                                        ::std::vec![
                                                            ::ethers::core::abi::ethabi::ParamType::String,
                                                            ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                                            ::ethers::core::abi::ethabi::ParamType::Tuple(
                                                                ::std::vec![
                                                                    ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                                                    ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                                                ],
                                                            ),
                                                            ::ethers::core::abi::ethabi::ParamType::Uint(256usize),
                                                            ::ethers::core::abi::ethabi::ParamType::Uint(256usize),
                                                        ],
                                                    ),
                                                ),
                                            ),
                                            ::ethers::core::abi::ethabi::ParamType::Array(
                                                ::std::boxed::Box::new(
                                                    ::ethers::core::abi::ethabi::ParamType::Tuple(
                                                        ::std::vec![
                                                            ::ethers::core::abi::ethabi::ParamType::String,
                                                            ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                                            ::ethers::core::abi::ethabi::ParamType::Tuple(
                                                                ::std::vec![
                                                                    ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                                                    ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                                                ],
                                                            ),
                                                            ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                                            ::ethers::core::abi::ethabi::ParamType::Array(
                                                                ::std::boxed::Box::new(
                                                                    ::ethers::core::abi::ethabi::ParamType::Int(256usize),
                                                                ),
                                                            ),
                                                            ::ethers::core::abi::ethabi::ParamType::Array(
                                                                ::std::boxed::Box::new(
                                                                    ::ethers::core::abi::ethabi::ParamType::Int(256usize),
                                                                ),
                                                            ),
                                                        ],
                                                    ),
                                                ),
                                            ),
                                        ],
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("struct Model.Head"),
                                    ),
                                },
                            ],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::View,
                        },
                    ],
                ),
                (
                    ::std::borrow::ToOwned::to_owned("latestBlocks"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("latestBlocks"),
                            inputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint256"),
                                    ),
                                },
                            ],
                            outputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint256"),
                                    ),
                                },
                            ],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::View,
                        },
                    ],
                ),
                (
                    ::std::borrow::ToOwned::to_owned("sequence"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("sequence"),
                            inputs: ::std::vec![],
                            outputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint256"),
                                    ),
                                },
                            ],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::View,
                        },
                    ],
                ),
                (
                    ::std::borrow::ToOwned::to_owned("signal"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("signal"),
                            inputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::borrow::ToOwned::to_owned("action"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint8"),
                                    ),
                                },
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::borrow::ToOwned::to_owned("scalar"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint256"),
                                    ),
                                },
                            ],
                            outputs: ::std::vec![],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::NonPayable,
                        },
                    ],
                ),
                (
                    ::std::borrow::ToOwned::to_owned("signalMany"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("signalMany"),
                            inputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::borrow::ToOwned::to_owned("actions"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Array(
                                        ::std::boxed::Box::new(
                                            ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                        ),
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint8[]"),
                                    ),
                                },
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::borrow::ToOwned::to_owned("scalars"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Array(
                                        ::std::boxed::Box::new(
                                            ::ethers::core::abi::ethabi::ParamType::Uint(256usize),
                                        ),
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint256[]"),
                                    ),
                                },
                            ],
                            outputs: ::std::vec![],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::NonPayable,
                        },
                    ],
                ),
                (
                    ::std::borrow::ToOwned::to_owned("state"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("state"),
                            inputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint256"),
                                    ),
                                },
                            ],
                            outputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Int(256usize),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("int256"),
                                    ),
                                },
                            ],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::View,
                        },
                    ],
                ),
            ]),
            events: ::core::convert::From::from([
                (
                    ::std::borrow::ToOwned::to_owned("SignaledEvent"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Event {
                            name: ::std::borrow::ToOwned::to_owned("SignaledEvent"),
                            inputs: ::std::vec![
                                ::ethers::core::abi::ethabi::EventParam {
                                    name: ::std::borrow::ToOwned::to_owned("role"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                    indexed: true,
                                },
                                ::ethers::core::abi::ethabi::EventParam {
                                    name: ::std::borrow::ToOwned::to_owned("actionId"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(8usize),
                                    indexed: true,
                                },
                                ::ethers::core::abi::ethabi::EventParam {
                                    name: ::std::borrow::ToOwned::to_owned("scalar"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    indexed: true,
                                },
                                ::ethers::core::abi::ethabi::EventParam {
                                    name: ::std::borrow::ToOwned::to_owned("sequence"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    indexed: false,
                                },
                            ],
                            anonymous: false,
                        },
                    ],
                ),
            ]),
            errors: ::std::collections::BTreeMap::new(),
            receive: false,
            fallback: false,
        }
    }
    ///The parsed JSON ABI of the contract.
    pub static MYSTATEMACHINE_ABI: ::ethers::contract::Lazy<::ethers::core::abi::Abi> = ::ethers::contract::Lazy::new(
        __abi,
    );
    #[rustfmt::skip]
    const __BYTECODE: &[u8] = b"`\0`\x02\x81\x90Ua\x01\xC0`@R`\x80\x81\x81R`\xA0\x82\x90R`\xC0\x82\x90R`\xE0\x82\x90Ra\x01\0\x82\x90Ra\x01 \x82\x90Ra\x01@\x82\x90Ra\x01`\x82\x90Ra\x01\x80\x82\x90Ra\x01\xA0\x91\x90\x91Rb\0\0U\x90`\x03\x90`\nb\0\x13\xAAV[P`@\x80Q`\x01\x80\x82R\x81\x83\x01\x90\x92R\x90` \x80\x83\x01\x90\x806\x837PP\x81Qb\0\0\x87\x92`\r\x92P` \x01\x90b\0\x13\xF2V[P4\x80\x15b\0\0\x95W`\0\x80\xFD[P`@\x80Q\x80\x82\x01\x82R`\x06\x81Re\x07\x06\xC6\x166S`\xD4\x1B` \x80\x83\x01\x91\x90\x91R\x82Q\x80\x84\x01\x90\x93R`\x01\x83R`\x02\x90\x83\x01Rb\0\0\xD9\x91`\0\x90`\x03\x90b\0\r\xE2V[P`@\x80Q\x80\x82\x01\x90\x91R`\x04\x81Rc\x07G\x86\xE3`\xE4\x1B` \x82\x01Rb\0\x01\x1A\x90`\x01`\0\x80`@\x80Q\x80\x82\x01\x90\x91R`\0\x81R`\x01` \x82\x01Rb\0\x0E\xFDV[P`@\x80Q\x80\x82\x01\x90\x91R`\x04\x81Rctxn1`\xE0\x1B` \x82\x01Rb\0\x01[\x90`\x01\x80`\0`@\x80Q\x80\x82\x01\x90\x91R`\x02\x81R`\x01` \x82\x01Rb\0\x0E\xFDV[P`@\x80Q\x80\x82\x01\x90\x91R`\x04\x81Rc:<7\x19`\xE1\x1B` \x82\x01Rb\0\x01\x9D\x90`\x01`\x02`\0`@\x80Q\x80\x82\x01\x90\x91R`\0\x81R`\x03` \x82\x01Rb\0\x0E\xFDV[P`@\x80Q\x80\x82\x01\x90\x91R`\x04\x81Rctxn3`\xE0\x1B` \x82\x01Rb\0\x01\xDF\x90`\x01`\x03`\0`@\x80Q\x80\x82\x01\x90\x91R`\x02\x81R`\x03` \x82\x01Rb\0\x0E\xFDV[Pb\0\x04\xBD`\x01\x80`\0\x81T\x81\x10b\0\x01\xFCWb\0\x01\xFCb\0\x14]V[\x90`\0R` `\0 \x90`\x06\x02\x01`@Q\x80`\xC0\x01`@R\x90\x81`\0\x82\x01\x80Tb\0\x02'\x90b\0\x14sV[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Tb\0\x02U\x90b\0\x14sV[\x80\x15b\0\x02\xA6W\x80`\x1F\x10b\0\x02zWa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91b\0\x02\xA6V[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11b\0\x02\x88W\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x84\x16\x81\x84\x01R\x81\x85\x01R`\x03\x85\x01T\x90\x92\x16``\x84\x01R`\x04\x84\x01\x80T\x83Q\x81\x84\x02\x81\x01\x84\x01\x90\x94R\x80\x84R`\x80\x90\x94\x01\x93\x90\x91\x83\x01\x82\x82\x80\x15b\0\x03>W` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11b\0\x03)W[PPPPP\x81R` \x01`\x05\x82\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15b\0\x03\x98W` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11b\0\x03\x83W[PPPPP\x81RPP`\0\x80\x81T\x81\x10b\0\x03\xB7Wb\0\x03\xB7b\0\x14]V[\x90`\0R` `\0 \x90`\x05\x02\x01`@Q\x80`\xA0\x01`@R\x90\x81`\0\x82\x01\x80Tb\0\x03\xE2\x90b\0\x14sV[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Tb\0\x04\x10\x90b\0\x14sV[\x80\x15b\0\x04aW\x80`\x1F\x10b\0\x045Wa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91b\0\x04aV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11b\0\x04CW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x90\x93\x16\x91\x83\x01\x91\x90\x91R\x82\x01R`\x03\x82\x01T``\x82\x01R`\x04\x90\x91\x01T`\x80\x90\x91\x01Rb\0\x11UV[b\0\x07\xA0`\x03`\0\x80\x81T\x81\x10b\0\x04\xD9Wb\0\x04\xD9b\0\x14]V[\x90`\0R` `\0 \x90`\x05\x02\x01`@Q\x80`\xA0\x01`@R\x90\x81`\0\x82\x01\x80Tb\0\x05\x04\x90b\0\x14sV[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Tb\0\x052\x90b\0\x14sV[\x80\x15b\0\x05\x83W\x80`\x1F\x10b\0\x05WWa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91b\0\x05\x83V[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11b\0\x05eW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x81\x01T`\xFF\x90\x81\x16` \x80\x85\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x87\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x90\x93\x16\x91\x83\x01\x91\x90\x91R\x83\x01R`\x03\x83\x01T``\x83\x01R`\x04\x90\x92\x01T`\x80\x90\x91\x01R\x80T\x81\x90\x81\x10b\0\x05\xEFWb\0\x05\xEFb\0\x14]V[\x90`\0R` `\0 \x90`\x06\x02\x01`@Q\x80`\xC0\x01`@R\x90\x81`\0\x82\x01\x80Tb\0\x06\x1A\x90b\0\x14sV[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Tb\0\x06H\x90b\0\x14sV[\x80\x15b\0\x06\x99W\x80`\x1F\x10b\0\x06mWa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91b\0\x06\x99V[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11b\0\x06{W\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x84\x16\x81\x84\x01R\x81\x85\x01R`\x03\x85\x01T\x90\x92\x16``\x84\x01R`\x04\x84\x01\x80T\x83Q\x81\x84\x02\x81\x01\x84\x01\x90\x94R\x80\x84R`\x80\x90\x94\x01\x93\x90\x91\x83\x01\x82\x82\x80\x15b\0\x071W` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11b\0\x07\x1CW[PPPPP\x81R` \x01`\x05\x82\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15b\0\x07\x8BW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11b\0\x07vW[PPPPP\x81RPPb\0\x11\xF1` \x1B` \x1CV[b\0\n~`\x03`\x01`\x02\x81T\x81\x10b\0\x07\xBDWb\0\x07\xBDb\0\x14]V[\x90`\0R` `\0 \x90`\x06\x02\x01`@Q\x80`\xC0\x01`@R\x90\x81`\0\x82\x01\x80Tb\0\x07\xE8\x90b\0\x14sV[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Tb\0\x08\x16\x90b\0\x14sV[\x80\x15b\0\x08gW\x80`\x1F\x10b\0\x08;Wa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91b\0\x08gV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11b\0\x08IW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x84\x16\x81\x84\x01R\x81\x85\x01R`\x03\x85\x01T\x90\x92\x16``\x84\x01R`\x04\x84\x01\x80T\x83Q\x81\x84\x02\x81\x01\x84\x01\x90\x94R\x80\x84R`\x80\x90\x94\x01\x93\x90\x91\x83\x01\x82\x82\x80\x15b\0\x08\xFFW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11b\0\x08\xEAW[PPPPP\x81R` \x01`\x05\x82\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15b\0\tYW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11b\0\tDW[PPPPP\x81RPP`\0\x80\x81T\x81\x10b\0\txWb\0\txb\0\x14]V[\x90`\0R` `\0 \x90`\x05\x02\x01`@Q\x80`\xA0\x01`@R\x90\x81`\0\x82\x01\x80Tb\0\t\xA3\x90b\0\x14sV[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Tb\0\t\xD1\x90b\0\x14sV[\x80\x15b\0\n\"W\x80`\x1F\x10b\0\t\xF6Wa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91b\0\n\"V[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11b\0\n\x04W\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x90\x93\x16\x91\x83\x01\x91\x90\x91R\x82\x01R`\x03\x82\x01T``\x82\x01R`\x04\x90\x91\x01T`\x80\x90\x91\x01Rb\0\x12\x88V[b\0\rf`\x01`\0\x80\x81T\x81\x10b\0\n\x9AWb\0\n\x9Ab\0\x14]V[\x90`\0R` `\0 \x90`\x05\x02\x01`@Q\x80`\xA0\x01`@R\x90\x81`\0\x82\x01\x80Tb\0\n\xC5\x90b\0\x14sV[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Tb\0\n\xF3\x90b\0\x14sV[\x80\x15b\0\x0BDW\x80`\x1F\x10b\0\x0B\x18Wa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91b\0\x0BDV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11b\0\x0B&W\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x81\x01T`\xFF\x90\x81\x16` \x80\x85\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x87\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x90\x93\x16\x91\x83\x01\x91\x90\x91R\x83\x01R`\x03\x80\x84\x01T``\x84\x01R`\x04\x90\x93\x01T`\x80\x90\x92\x01\x91\x90\x91R\x80T\x90\x91\x90\x81\x10b\0\x0B\xB5Wb\0\x0B\xB5b\0\x14]V[\x90`\0R` `\0 \x90`\x06\x02\x01`@Q\x80`\xC0\x01`@R\x90\x81`\0\x82\x01\x80Tb\0\x0B\xE0\x90b\0\x14sV[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Tb\0\x0C\x0E\x90b\0\x14sV[\x80\x15b\0\x0C_W\x80`\x1F\x10b\0\x0C3Wa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91b\0\x0C_V[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11b\0\x0CAW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x84\x16\x81\x84\x01R\x81\x85\x01R`\x03\x85\x01T\x90\x92\x16``\x84\x01R`\x04\x84\x01\x80T\x83Q\x81\x84\x02\x81\x01\x84\x01\x90\x94R\x80\x84R`\x80\x90\x94\x01\x93\x90\x91\x83\x01\x82\x82\x80\x15b\0\x0C\xF7W` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11b\0\x0C\xE2W[PPPPP\x81R` \x01`\x05\x82\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15b\0\rQW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11b\0\r<W[PPPPP\x81RPPb\0\x13\x13` \x1B` \x1CV[`\0[`\x01`\xFF\x82\x16\x10\x15b\0\r\xDBW`\0\x81`\xFF\x16\x81T\x81\x10b\0\r\x8FWb\0\r\x8Fb\0\x14]V[\x90`\0R` `\0 \x90`\x05\x02\x01`\x03\x01T`\r\x82`\xFF\x16\x81T\x81\x10b\0\r\xBAWb\0\r\xBAb\0\x14]V[`\0\x91\x82R` \x90\x91 \x01U\x80b\0\r\xD2\x81b\0\x14\xC5V[\x91PPb\0\riV[Pb\0\x160V[b\0\x0E(`@\x80Q`\xA0\x81\x01\x82R``\x81R`\0` \x80\x83\x01\x82\x90R\x83Q\x80\x85\x01\x85R\x82\x81R\x90\x81\x01\x91\x90\x91R\x90\x91\x82\x01\x90\x81R` \x01`\0\x81R` \x01`\0\x81RP\x90V[`@\x80Q`\xA0\x81\x01\x82R\x86\x81R`\0\x80T`\xFF\x81\x16` \x84\x01R\x92\x82\x01\x85\x90R``\x82\x01\x87\x90R`\x80\x82\x01\x86\x90R`\x01\x83\x01\x81U\x80R\x80Q\x90\x91\x82\x91`\x05\x90\x91\x02\x7F)\r\xEC\xD9T\x8Bb\xA8\xD6\x03E\xA9\x888o\xC8K\xA6\xBC\x95H@\x08\xF66/\x93\x16\x0E\xF3\xE5c\x01\x90\x81\x90b\0\x0E\x9A\x90\x82b\0\x15:V[P` \x82\x81\x01Q`\x01\x83\x01\x80T`\xFF\x92\x83\x16`\xFF\x19\x90\x91\x16\x17\x90U`@\x84\x01Q\x80Q`\x02\x85\x01\x80T\x92\x90\x94\x01Q\x83\x16a\x01\0\x02a\xFF\xFF\x19\x90\x92\x16\x92\x16\x91\x90\x91\x17\x17\x90U``\x82\x01Q`\x03\x82\x01U`\x80\x90\x91\x01Q`\x04\x90\x91\x01U\x90P\x94\x93PPPPV[b\0\x0FH`@\x80Q`\xC0\x81\x01\x82R``\x81R`\0` \x80\x83\x01\x82\x90R\x83Q\x80\x85\x01\x85R\x82\x81R\x90\x81\x01\x91\x90\x91R\x90\x91\x82\x01\x90\x81R`\0` \x82\x01R```@\x82\x01\x81\x90R\x90\x81\x01R\x90V[`\x01T`\xFF\x85\x81\x16\x91\x16\x14b\0\x0F\xA5W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x1C`$\x82\x01R\x7Ftransaction => enum mismatch\0\0\0\0`D\x82\x01R`d\x01[`@Q\x80\x91\x03\x90\xFD[`\0`@Q\x80`\xC0\x01`@R\x80\x88\x81R` \x01\x86`\xFF\x16\x81R` \x01\x84\x81R` \x01\x85`\xFF\x16\x81R` \x01\x87`\xFF\x16`\x01`\x01`@\x1B\x03\x81\x11\x15b\0\x0F\xEEWb\0\x0F\xEEb\0\x14GV[`@Q\x90\x80\x82R\x80` \x02` \x01\x82\x01`@R\x80\x15b\0\x10\x18W\x81` \x01` \x82\x02\x806\x837\x01\x90P[P\x81R` \x01\x87`\xFF\x16`\x01`\x01`@\x1B\x03\x81\x11\x15b\0\x10<Wb\0\x10<b\0\x14GV[`@Q\x90\x80\x82R\x80` \x02` \x01\x82\x01`@R\x80\x15b\0\x10fW\x81` \x01` \x82\x02\x806\x837\x01\x90P[P\x90R`\x01\x80T\x80\x82\x01\x82U`\0\x91\x90\x91R\x81Q\x91\x92P\x82\x91`\x06\x90\x91\x02\x7F\xB1\x0E-Rv\x12\x07;&\xEE\xCD\xFDq~j2\x0C\xF4KJ\xFA\xC2\xB0s-\x9F\xCB\xE2\xB7\xFA\x0C\xF6\x01\x90\x81\x90b\0\x10\xB5\x90\x82b\0\x15:V[P` \x82\x81\x01Q`\x01\x83\x01\x80T`\xFF\x92\x83\x16`\xFF\x19\x91\x82\x16\x17\x90\x91U`@\x85\x01Q\x80Q`\x02\x86\x01\x80T\x92\x86\x01Q\x85\x16a\x01\0\x02a\xFF\xFF\x19\x90\x93\x16\x91\x85\x16\x91\x90\x91\x17\x91\x90\x91\x17\x90U``\x85\x01Q`\x03\x85\x01\x80T\x91\x90\x93\x16\x91\x16\x17\x90U`\x80\x83\x01Q\x80Qb\0\x11)\x92`\x04\x85\x01\x92\x01\x90b\0\x13\xF2V[P`\xA0\x82\x01Q\x80Qb\0\x11G\x91`\x05\x84\x01\x91` \x90\x91\x01\x90b\0\x13\xF2V[P\x91\x98\x97PPPPPPPPV[`\0\x83\x13b\0\x11\x96W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x12`$\x82\x01R`\0\x80Q` b\0'\xD0\x839\x81Q\x91R`D\x82\x01R`d\x01b\0\x0F\x9CV[\x82`\x01\x83` \x01Q`\xFF\x16\x81T\x81\x10b\0\x11\xB4Wb\0\x11\xB4b\0\x14]V[\x90`\0R` `\0 \x90`\x06\x02\x01`\x04\x01\x82` \x01Q`\xFF\x16\x81T\x81\x10b\0\x11\xE0Wb\0\x11\xE0b\0\x14]V[`\0\x91\x82R` \x90\x91 \x01UPPPV[`\0\x83\x13b\0\x122W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x12`$\x82\x01R`\0\x80Q` b\0'\xD0\x839\x81Q\x91R`D\x82\x01R`d\x01b\0\x0F\x9CV[b\0\x12?\x83`\0b\0\x16\x06V[`\x01\x82` \x01Q`\xFF\x16\x81T\x81\x10b\0\x12\\Wb\0\x12\\b\0\x14]V[\x90`\0R` `\0 \x90`\x06\x02\x01`\x04\x01\x83` \x01Q`\xFF\x16\x81T\x81\x10b\0\x11\xE0Wb\0\x11\xE0b\0\x14]V[`\0\x83\x13b\0\x12\xC9W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x12`$\x82\x01R`\0\x80Q` b\0'\xD0\x839\x81Q\x91R`D\x82\x01R`d\x01b\0\x0F\x9CV[\x82`\x01\x83` \x01Q`\xFF\x16\x81T\x81\x10b\0\x12\xE7Wb\0\x12\xE7b\0\x14]V[\x90`\0R` `\0 \x90`\x06\x02\x01`\x05\x01\x82` \x01Q`\xFF\x16\x81T\x81\x10b\0\x11\xE0Wb\0\x11\xE0b\0\x14]V[`\0\x83\x13b\0\x13TW`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x12`$\x82\x01R`\0\x80Q` b\0'\xD0\x839\x81Q\x91R`D\x82\x01R`d\x01b\0\x0F\x9CV[b\0\x13a\x83`\0b\0\x16\x06V[`\x01\x82` \x01Q`\xFF\x16\x81T\x81\x10b\0\x13~Wb\0\x13~b\0\x14]V[\x90`\0R` `\0 \x90`\x06\x02\x01`\x05\x01\x83` \x01Q`\xFF\x16\x81T\x81\x10b\0\x11\xE0Wb\0\x11\xE0b\0\x14]V[\x82`\n\x81\x01\x92\x82\x15b\0\x13\xE0W\x91` \x02\x82\x01[\x82\x81\x11\x15b\0\x13\xE0W\x82Q\x82\x90`\xFF\x16\x90U\x91` \x01\x91\x90`\x01\x01\x90b\0\x13\xBEV[Pb\0\x13\xEE\x92\x91Pb\0\x140V[P\x90V[\x82\x80T\x82\x82U\x90`\0R` `\0 \x90\x81\x01\x92\x82\x15b\0\x13\xE0W\x91` \x02\x82\x01[\x82\x81\x11\x15b\0\x13\xE0W\x82Q\x82U\x91` \x01\x91\x90`\x01\x01\x90b\0\x14\x13V[[\x80\x82\x11\x15b\0\x13\xEEW`\0\x81U`\x01\x01b\0\x141V[cNH{q`\xE0\x1B`\0R`A`\x04R`$`\0\xFD[cNH{q`\xE0\x1B`\0R`2`\x04R`$`\0\xFD[`\x01\x81\x81\x1C\x90\x82\x16\x80b\0\x14\x88W`\x7F\x82\x16\x91P[` \x82\x10\x81\x03b\0\x14\xA9WcNH{q`\xE0\x1B`\0R`\"`\x04R`$`\0\xFD[P\x91\x90PV[cNH{q`\xE0\x1B`\0R`\x11`\x04R`$`\0\xFD[`\0`\xFF\x82\x16`\xFF\x81\x03b\0\x14\xDEWb\0\x14\xDEb\0\x14\xAFV[`\x01\x01\x92\x91PPV[`\x1F\x82\x11\x15b\0\x155W`\0\x81\x81R` \x81 `\x1F\x85\x01`\x05\x1C\x81\x01` \x86\x10\x15b\0\x15\x10WP\x80[`\x1F\x85\x01`\x05\x1C\x82\x01\x91P[\x81\x81\x10\x15b\0\x151W\x82\x81U`\x01\x01b\0\x15\x1CV[PPP[PPPV[\x81Q`\x01`\x01`@\x1B\x03\x81\x11\x15b\0\x15VWb\0\x15Vb\0\x14GV[b\0\x15n\x81b\0\x15g\x84Tb\0\x14sV[\x84b\0\x14\xE7V[` \x80`\x1F\x83\x11`\x01\x81\x14b\0\x15\xA6W`\0\x84\x15b\0\x15\x8DWP\x85\x83\x01Q[`\0\x19`\x03\x86\x90\x1B\x1C\x19\x16`\x01\x85\x90\x1B\x17\x85Ub\0\x151V[`\0\x85\x81R` \x81 `\x1F\x19\x86\x16\x91[\x82\x81\x10\x15b\0\x15\xD7W\x88\x86\x01Q\x82U\x94\x84\x01\x94`\x01\x90\x91\x01\x90\x84\x01b\0\x15\xB6V[P\x85\x82\x10\x15b\0\x15\xF6W\x87\x85\x01Q`\0\x19`\x03\x88\x90\x1B`\xF8\x16\x1C\x19\x16\x81U[PPPPP`\x01\x90\x81\x1B\x01\x90UPV[\x81\x81\x03`\0\x83\x12\x80\x15\x83\x83\x13\x16\x83\x83\x12\x82\x16\x17\x15b\0\x16)Wb\0\x16)b\0\x14\xAFV[P\x92\x91PPV[a\x11\x90\x80b\0\x16@`\09`\0\xF3\xFE`\x80`@R4\x80\x15a\0\x10W`\0\x80\xFD[P`\x046\x10a\0bW`\x005`\xE0\x1C\x80c>OI\xE6\x14a\0gW\x80cR\x9D\x15\xCC\x14a\0\x8DW\x80cn\xE3v\xE6\x14a\0\x96W\x80c\xD0Imj\x14a\0\xA9W\x80c\xDD\xC3\xB1\x87\x14a\0\xBEW\x80c\xFF\xF0\x1F\xE2\x14a\0\xD3W[`\0\x80\xFD[a\0za\0u6`\x04a\x0CDV[a\0\xE6V[`@Q\x90\x81R` \x01[`@Q\x80\x91\x03\x90\xF3[a\0z`\x02T\x81V[a\0za\0\xA46`\x04a\x0CDV[a\x01\x07V[a\0\xB1a\x01\x1EV[`@Qa\0\x84\x91\x90a\x0EOV[a\0\xD1a\0\xCC6`\x04a\x0F\x04V[a\x04\xDCV[\0[a\0\xD1a\0\xE16`\x04a\x0FzV[a\x04\xF3V[`\r\x81\x81T\x81\x10a\0\xF6W`\0\x80\xFD[`\0\x91\x82R` \x90\x91 \x01T\x90P\x81V[`\x03\x81`\n\x81\x10a\x01\x17W`\0\x80\xFD[\x01T\x90P\x81V[a\x01&a\x0B\xF0V[`@\x80Qa\x01\xE0\x81\x01\x90\x91R\x80`\xA0\x81\x01`\x03`\n\x82\x82\x82` \x02\x82\x01\x91[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x01EWPPPPP\x81R` \x01`\x02T\x81R` \x01`\r\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15a\x01\xB6W` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x01\xA2W[PPPPP\x81R` \x01`\0\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01`\0\x90[\x82\x82\x10\x15a\x02\xF4W\x83\x82\x90`\0R` `\0 \x90`\x05\x02\x01`@Q\x80`\xA0\x01`@R\x90\x81`\0\x82\x01\x80Ta\x02\x15\x90a\x0F\xE6V[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Ta\x02A\x90a\x0F\xE6V[\x80\x15a\x02\x8EW\x80`\x1F\x10a\x02cWa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91a\x02\x8EV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11a\x02qW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x81\x01T`\xFF\x90\x81\x16` \x80\x85\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x87\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x90\x93\x16\x83\x83\x01R\x84\x01\x91\x90\x91R`\x03\x84\x01T``\x84\x01R`\x04\x90\x93\x01T`\x80\x90\x92\x01\x91\x90\x91R\x91\x83R\x92\x01\x91\x01a\x01\xE2V[PPPP\x81R` \x01`\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01`\0\x90[\x82\x82\x10\x15a\x04\xD1W\x83\x82\x90`\0R` `\0 \x90`\x06\x02\x01`@Q\x80`\xC0\x01`@R\x90\x81`\0\x82\x01\x80Ta\x03R\x90a\x0F\xE6V[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Ta\x03~\x90a\x0F\xE6V[\x80\x15a\x03\xCBW\x80`\x1F\x10a\x03\xA0Wa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91a\x03\xCBV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11a\x03\xAEW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x84\x16\x81\x84\x01R\x81\x85\x01R`\x03\x85\x01T\x90\x92\x16``\x84\x01R`\x04\x84\x01\x80T\x83Q\x81\x84\x02\x81\x01\x84\x01\x90\x94R\x80\x84R`\x80\x90\x94\x01\x93\x90\x91\x83\x01\x82\x82\x80\x15a\x04aW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x04MW[PPPPP\x81R` \x01`\x05\x82\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15a\x04\xB9W` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x04\xA5W[PPPPP\x81RPP\x81R` \x01\x90`\x01\x01\x90a\x03\x1FV[PPP\x91RP\x91\x90PV[a\x04\xE6\x82\x82a\x05\xBCV[a\x04\xEFCa\x08^V[PPV[\x82\x81\x14a\x05GW`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x1C`$\x82\x01R\x7FModelRegistry: invalid input\0\0\0\0`D\x82\x01R`d\x01[`@Q\x80\x91\x03\x90\xFD[`\0[\x83\x81\x10\x15a\x05\xACWa\x05\x9A\x85\x85\x83\x81\x81\x10a\x05gWa\x05ga\x10 V[\x90P` \x02\x01` \x81\x01\x90a\x05|\x91\x90a\x106V[\x84\x84\x84\x81\x81\x10a\x05\x8EWa\x05\x8Ea\x10 V[\x90P` \x02\x015a\x05\xBCV[\x80a\x05\xA4\x81a\x10nV[\x91PPa\x05JV[Pa\x05\xB6Ca\x08^V[PPPPV[`\0`\x01\x83`\xFF\x16\x81T\x81\x10a\x05\xD4Wa\x05\xD4a\x10 V[\x90`\0R` `\0 \x90`\x06\x02\x01`@Q\x80`\xC0\x01`@R\x90\x81`\0\x82\x01\x80Ta\x05\xFD\x90a\x0F\xE6V[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Ta\x06)\x90a\x0F\xE6V[\x80\x15a\x06vW\x80`\x1F\x10a\x06KWa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91a\x06vV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11a\x06YW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x84\x16\x81\x84\x01R\x81\x85\x01R`\x03\x85\x01T\x90\x92\x16``\x84\x01R`\x04\x84\x01\x80T\x83Q\x81\x84\x02\x81\x01\x84\x01\x90\x94R\x80\x84R`\x80\x90\x94\x01\x93\x90\x91\x83\x01\x82\x82\x80\x15a\x07\x0CW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x06\xF8W[PPPPP\x81R` \x01`\x05\x82\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15a\x07dW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x07PW[PPPPP\x81RPP\x90Pa\x07x\x81a\x08\xBFV[\x15a\x07\xB1W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\t`$\x82\x01Rh\x1A[\x9A\x1AX\x9A]\x19Y`\xBA\x1B`D\x82\x01R`d\x01a\x05>V[\x80` \x01Q`\xFF\x16\x83`\xFF\x16\x14a\x07\xCAWa\x07\xCAa\x10\x87V[`\0[`\0T`\xFF\x90\x81\x16\x90\x82\x16\x10\x15a\x07\xFBWa\x07\xE9\x81\x83\x85a\t\xF9V[\x80a\x07\xF3\x81a\x10\x9DV[\x91PPa\x07\xCDV[P`\x02\x80T\x90`\0a\x08\x0C\x83a\x10nV[\x91\x90PUP\x81\x83`\xFF\x16\x82``\x01Q`\xFF\x16\x7FP\xE4\xA5+\x07r\xBE\xD9\xF0j}?}\xFAf\xD76@\x06z\\\xC7zs\xC2EV\xCC\xC9\0\xFA\x08`\x02T`@Qa\x08Q\x91\x81R` \x01\x90V[`@Q\x80\x91\x03\x90\xA4PPPV[`\0[`\t\x81`\xFF\x16\x10\x15a\x08\xB9W`\x03a\x08z\x82`\x01a\x10\xBCV[`\xFF\x16`\n\x81\x10a\x08\x8DWa\x08\x8Da\x10 V[\x01T`\x03\x82`\xFF\x16`\n\x81\x10a\x08\xA5Wa\x08\xA5a\x10 V[\x01U\x80a\x08\xB1\x81a\x10\x9DV[\x91PPa\x08aV[P`\x0CUV[`\0\x80[`\x01`\xFF\x82\x16\x10\x15a\t\xF0W\x82`\xA0\x01Q\x81`\xFF\x16\x81Q\x81\x10a\x08\xE8Wa\x08\xE8a\x10 V[` \x02` \x01\x01Q`\0\x14a\t\xDEW`\0\x83`\xA0\x01Q\x82`\xFF\x16\x81Q\x81\x10a\t\x12Wa\t\x12a\x10 V[` \x02` \x01\x01Q\x12\x15a\t\x81W`\0\x83`\xA0\x01Q\x82`\xFF\x16\x81Q\x81\x10a\t;Wa\t;a\x10 V[` \x02` \x01\x01Q`\r\x83`\xFF\x16\x81T\x81\x10a\tYWa\tYa\x10 V[\x90`\0R` `\0 \x01Ta\tn\x91\x90a\x10\xDBV[\x12a\t|WP`\x01\x92\x91PPV[a\t\xDEV[`\0\x83`\xA0\x01Q\x82`\xFF\x16\x81Q\x81\x10a\t\x9CWa\t\x9Ca\x10 V[` \x02` \x01\x01Q`\r\x83`\xFF\x16\x81T\x81\x10a\t\xBAWa\t\xBAa\x10 V[\x90`\0R` `\0 \x01Ta\t\xCF\x91\x90a\x11\x03V[\x12\x15a\t\xDEWP`\x01\x92\x91PPV[\x80a\t\xE8\x81a\x10\x9DV[\x91PPa\x08\xC3V[P`\0\x92\x91PPV[`\0\x81\x11a\n:W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x0E`$\x82\x01Rm4\xB7;0\xB64\xB2\x109\xB1\xB0\xB60\xB9`\x91\x1B`D\x82\x01R`d\x01a\x05>V[\x81`\x80\x01Q\x83`\xFF\x16\x81Q\x81\x10a\nSWa\nSa\x10 V[` \x02` \x01\x01Q`\0\x14a\x0B\xEBW\x80\x82`\x80\x01Q\x84`\xFF\x16\x81Q\x81\x10a\n|Wa\n|a\x10 V[` \x02` \x01\x01Qa\n\x8E\x91\x90a\x11*V[`\r\x84`\xFF\x16\x81T\x81\x10a\n\xA4Wa\n\xA4a\x10 V[\x90`\0R` `\0 \x01Ta\n\xB9\x91\x90a\x10\xDBV[`\r\x84`\xFF\x16\x81T\x81\x10a\n\xCFWa\n\xCFa\x10 V[\x90`\0R` `\0 \x01\x81\x90UP`\0`\r\x84`\xFF\x16\x81T\x81\x10a\n\xF5Wa\n\xF5a\x10 V[\x90`\0R` `\0 \x01T\x12\x15a\x0B:W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\t`$\x82\x01Rhunderflow`\xB8\x1B`D\x82\x01R`d\x01a\x05>V[`\0\x80\x84`\xFF\x16\x81T\x81\x10a\x0BQWa\x0BQa\x10 V[\x90`\0R` `\0 \x90`\x05\x02\x01`\x04\x01T\x11\x15a\x0B\xEBW`\0\x83`\xFF\x16\x81T\x81\x10a\x0B\x7FWa\x0B\x7Fa\x10 V[\x90`\0R` `\0 \x90`\x05\x02\x01`\x04\x01T`\r\x84`\xFF\x16\x81T\x81\x10a\x0B\xA7Wa\x0B\xA7a\x10 V[\x90`\0R` `\0 \x01T\x13\x15a\x0B\xEBW`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x08`$\x82\x01Rgoverflow`\xC0\x1B`D\x82\x01R`d\x01a\x05>V[PPPV[`@Q\x80`\xA0\x01`@R\x80a\x0C\x03a\x0C%V[\x81R` \x01`\0\x81R` \x01``\x81R` \x01``\x81R` \x01``\x81RP\x90V[`@Q\x80a\x01@\x01`@R\x80`\n\x90` \x82\x02\x806\x837P\x91\x92\x91PPV[`\0` \x82\x84\x03\x12\x15a\x0CVW`\0\x80\xFD[P5\x91\x90PV[`\0\x81Q\x80\x84R` \x80\x85\x01\x94P\x80\x84\x01`\0[\x83\x81\x10\x15a\x0C\x8DW\x81Q\x87R\x95\x82\x01\x95\x90\x82\x01\x90`\x01\x01a\x0CqV[P\x94\x95\x94PPPPPV[`\0\x81Q\x80\x84R`\0[\x81\x81\x10\x15a\x0C\xBEW` \x81\x85\x01\x81\x01Q\x86\x83\x01\x82\x01R\x01a\x0C\xA2V[P`\0` \x82\x86\x01\x01R` `\x1F\x19`\x1F\x83\x01\x16\x85\x01\x01\x91PP\x92\x91PPV[`\0\x81Q\x80\x84R` \x80\x85\x01\x80\x81\x96P\x83`\x05\x1B\x81\x01\x91P\x82\x86\x01`\0[\x85\x81\x10\x15a\r{W\x82\x84\x03\x89R\x81Q`\xC0\x81Q\x81\x87Ra\r\x1E\x82\x88\x01\x82a\x0C\x98V[\x91PP`\xFF\x87\x83\x01Q\x16\x87\x87\x01R`@\x80\x83\x01Qa\rN\x82\x89\x01\x82\x80Q`\xFF\x90\x81\x16\x83R` \x91\x82\x01Q\x16\x91\x01RV[PP``\x82\x01Q`\x80\x87\x81\x01\x91\x90\x91R\x90\x91\x01Q`\xA0\x90\x95\x01\x94\x90\x94R\x97\x84\x01\x97\x90\x84\x01\x90`\x01\x01a\x0C\xFCV[P\x91\x97\x96PPPPPPPV[`\0\x81Q\x80\x84R` \x80\x85\x01\x80\x81\x96P\x83`\x05\x1B\x81\x01\x91P\x82\x86\x01`\0[\x85\x81\x10\x15a\r{W\x82\x84\x03\x89R\x81Q`\xE0\x81Q\x81\x87Ra\r\xC8\x82\x88\x01\x82a\x0C\x98V[\x83\x89\x01Q`\xFF\x90\x81\x16\x89\x8B\x01R`@\x80\x86\x01Q\x80Q\x83\x16\x82\x8C\x01R` \x81\x01Q\x83\x16``\x8C\x01R\x92\x94P\x92P\x90P``\x84\x01Q\x91P`\x80\x81\x83\x16\x81\x8A\x01R\x80\x85\x01Q\x92PPP`\xA0\x87\x83\x03\x81\x89\x01Ra\x0E!\x83\x83a\x0C]V[\x93\x01Q\x87\x84\x03`\xC0\x89\x01R\x92\x91Pa\x0E;\x90P\x81\x83a\x0C]V[\x9A\x87\x01\x9A\x95PPP\x90\x84\x01\x90`\x01\x01a\r\xA6V[` \x80\x82R\x82Q`\0\x91\x90\x82\x84\x83\x01[`\n\x82\x10\x15a\x0E~W\x82Q\x81R\x91\x83\x01\x91`\x01\x91\x90\x91\x01\x90\x83\x01a\x0E_V[PPP\x83\x01Qa\x01`\x83\x01R`@\x83\x01Qa\x01\xC0a\x01\x80\x84\x01\x81\x90Ra\x0E\xA8a\x01\xE0\x85\x01\x83a\x0C]V[\x91P``\x85\x01Q`\x1F\x19\x80\x86\x85\x03\x01a\x01\xA0\x87\x01Ra\x0E\xC7\x84\x83a\x0C\xDEV[\x93P`\x80\x87\x01Q\x91P\x80\x86\x85\x03\x01\x83\x87\x01RPa\x0E\xE4\x83\x82a\r\x88V[\x96\x95PPPPPPV[\x805`\xFF\x81\x16\x81\x14a\x0E\xFFW`\0\x80\xFD[\x91\x90PV[`\0\x80`@\x83\x85\x03\x12\x15a\x0F\x17W`\0\x80\xFD[a\x0F \x83a\x0E\xEEV[\x94` \x93\x90\x93\x015\x93PPPV[`\0\x80\x83`\x1F\x84\x01\x12a\x0F@W`\0\x80\xFD[P\x815g\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x81\x11\x15a\x0FXW`\0\x80\xFD[` \x83\x01\x91P\x83` \x82`\x05\x1B\x85\x01\x01\x11\x15a\x0FsW`\0\x80\xFD[\x92P\x92\x90PV[`\0\x80`\0\x80`@\x85\x87\x03\x12\x15a\x0F\x90W`\0\x80\xFD[\x845g\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x80\x82\x11\x15a\x0F\xA8W`\0\x80\xFD[a\x0F\xB4\x88\x83\x89\x01a\x0F.V[\x90\x96P\x94P` \x87\x015\x91P\x80\x82\x11\x15a\x0F\xCDW`\0\x80\xFD[Pa\x0F\xDA\x87\x82\x88\x01a\x0F.V[\x95\x98\x94\x97P\x95PPPPV[`\x01\x81\x81\x1C\x90\x82\x16\x80a\x0F\xFAW`\x7F\x82\x16\x91P[` \x82\x10\x81\x03a\x10\x1AWcNH{q`\xE0\x1B`\0R`\"`\x04R`$`\0\xFD[P\x91\x90PV[cNH{q`\xE0\x1B`\0R`2`\x04R`$`\0\xFD[`\0` \x82\x84\x03\x12\x15a\x10HW`\0\x80\xFD[a\x10Q\x82a\x0E\xEEV[\x93\x92PPPV[cNH{q`\xE0\x1B`\0R`\x11`\x04R`$`\0\xFD[`\0`\x01\x82\x01a\x10\x80Wa\x10\x80a\x10XV[P`\x01\x01\x90V[cNH{q`\xE0\x1B`\0R`\x01`\x04R`$`\0\xFD[`\0`\xFF\x82\x16`\xFF\x81\x03a\x10\xB3Wa\x10\xB3a\x10XV[`\x01\x01\x92\x91PPV[`\xFF\x81\x81\x16\x83\x82\x16\x01\x90\x81\x11\x15a\x10\xD5Wa\x10\xD5a\x10XV[\x92\x91PPV[\x80\x82\x01\x82\x81\x12`\0\x83\x12\x80\x15\x82\x16\x82\x15\x82\x16\x17\x15a\x10\xFBWa\x10\xFBa\x10XV[PP\x92\x91PPV[\x81\x81\x03`\0\x83\x12\x80\x15\x83\x83\x13\x16\x83\x83\x12\x82\x16\x17\x15a\x11#Wa\x11#a\x10XV[P\x92\x91PPV[\x80\x82\x02`\0\x82\x12`\x01`\xFF\x1B\x84\x14\x16\x15a\x11FWa\x11Fa\x10XV[\x81\x81\x05\x83\x14\x82\x15\x17a\x10\xD5Wa\x10\xD5a\x10XV\xFE\xA2dipfsX\"\x12 C\xBA\x8BiP\xA9\xCDh\xD9h=\xA7\xA0\x08O(\xF8tvI#\xF9\r\xD4\xF7\xCF\x01}\x08MG\x94dsolcC\0\x08\x15\x003weight must be > 0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
    /// The bytecode of the contract.
    pub static MYSTATEMACHINE_BYTECODE: ::ethers::core::types::Bytes = ::ethers::core::types::Bytes::from_static(
        __BYTECODE,
    );
    #[rustfmt::skip]
    const __DEPLOYED_BYTECODE: &[u8] = b"`\x80`@R4\x80\x15a\0\x10W`\0\x80\xFD[P`\x046\x10a\0bW`\x005`\xE0\x1C\x80c>OI\xE6\x14a\0gW\x80cR\x9D\x15\xCC\x14a\0\x8DW\x80cn\xE3v\xE6\x14a\0\x96W\x80c\xD0Imj\x14a\0\xA9W\x80c\xDD\xC3\xB1\x87\x14a\0\xBEW\x80c\xFF\xF0\x1F\xE2\x14a\0\xD3W[`\0\x80\xFD[a\0za\0u6`\x04a\x0CDV[a\0\xE6V[`@Q\x90\x81R` \x01[`@Q\x80\x91\x03\x90\xF3[a\0z`\x02T\x81V[a\0za\0\xA46`\x04a\x0CDV[a\x01\x07V[a\0\xB1a\x01\x1EV[`@Qa\0\x84\x91\x90a\x0EOV[a\0\xD1a\0\xCC6`\x04a\x0F\x04V[a\x04\xDCV[\0[a\0\xD1a\0\xE16`\x04a\x0FzV[a\x04\xF3V[`\r\x81\x81T\x81\x10a\0\xF6W`\0\x80\xFD[`\0\x91\x82R` \x90\x91 \x01T\x90P\x81V[`\x03\x81`\n\x81\x10a\x01\x17W`\0\x80\xFD[\x01T\x90P\x81V[a\x01&a\x0B\xF0V[`@\x80Qa\x01\xE0\x81\x01\x90\x91R\x80`\xA0\x81\x01`\x03`\n\x82\x82\x82` \x02\x82\x01\x91[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x01EWPPPPP\x81R` \x01`\x02T\x81R` \x01`\r\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15a\x01\xB6W` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x01\xA2W[PPPPP\x81R` \x01`\0\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01`\0\x90[\x82\x82\x10\x15a\x02\xF4W\x83\x82\x90`\0R` `\0 \x90`\x05\x02\x01`@Q\x80`\xA0\x01`@R\x90\x81`\0\x82\x01\x80Ta\x02\x15\x90a\x0F\xE6V[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Ta\x02A\x90a\x0F\xE6V[\x80\x15a\x02\x8EW\x80`\x1F\x10a\x02cWa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91a\x02\x8EV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11a\x02qW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x81\x01T`\xFF\x90\x81\x16` \x80\x85\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x87\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x90\x93\x16\x83\x83\x01R\x84\x01\x91\x90\x91R`\x03\x84\x01T``\x84\x01R`\x04\x90\x93\x01T`\x80\x90\x92\x01\x91\x90\x91R\x91\x83R\x92\x01\x91\x01a\x01\xE2V[PPPP\x81R` \x01`\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01`\0\x90[\x82\x82\x10\x15a\x04\xD1W\x83\x82\x90`\0R` `\0 \x90`\x06\x02\x01`@Q\x80`\xC0\x01`@R\x90\x81`\0\x82\x01\x80Ta\x03R\x90a\x0F\xE6V[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Ta\x03~\x90a\x0F\xE6V[\x80\x15a\x03\xCBW\x80`\x1F\x10a\x03\xA0Wa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91a\x03\xCBV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11a\x03\xAEW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x84\x16\x81\x84\x01R\x81\x85\x01R`\x03\x85\x01T\x90\x92\x16``\x84\x01R`\x04\x84\x01\x80T\x83Q\x81\x84\x02\x81\x01\x84\x01\x90\x94R\x80\x84R`\x80\x90\x94\x01\x93\x90\x91\x83\x01\x82\x82\x80\x15a\x04aW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x04MW[PPPPP\x81R` \x01`\x05\x82\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15a\x04\xB9W` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x04\xA5W[PPPPP\x81RPP\x81R` \x01\x90`\x01\x01\x90a\x03\x1FV[PPP\x91RP\x91\x90PV[a\x04\xE6\x82\x82a\x05\xBCV[a\x04\xEFCa\x08^V[PPV[\x82\x81\x14a\x05GW`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x1C`$\x82\x01R\x7FModelRegistry: invalid input\0\0\0\0`D\x82\x01R`d\x01[`@Q\x80\x91\x03\x90\xFD[`\0[\x83\x81\x10\x15a\x05\xACWa\x05\x9A\x85\x85\x83\x81\x81\x10a\x05gWa\x05ga\x10 V[\x90P` \x02\x01` \x81\x01\x90a\x05|\x91\x90a\x106V[\x84\x84\x84\x81\x81\x10a\x05\x8EWa\x05\x8Ea\x10 V[\x90P` \x02\x015a\x05\xBCV[\x80a\x05\xA4\x81a\x10nV[\x91PPa\x05JV[Pa\x05\xB6Ca\x08^V[PPPPV[`\0`\x01\x83`\xFF\x16\x81T\x81\x10a\x05\xD4Wa\x05\xD4a\x10 V[\x90`\0R` `\0 \x90`\x06\x02\x01`@Q\x80`\xC0\x01`@R\x90\x81`\0\x82\x01\x80Ta\x05\xFD\x90a\x0F\xE6V[\x80`\x1F\x01` \x80\x91\x04\x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80Ta\x06)\x90a\x0F\xE6V[\x80\x15a\x06vW\x80`\x1F\x10a\x06KWa\x01\0\x80\x83T\x04\x02\x83R\x91` \x01\x91a\x06vV[\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R\x90`\x01\x01\x90` \x01\x80\x83\x11a\x06YW\x82\x90\x03`\x1F\x16\x82\x01\x91[PPP\x91\x83RPP`\x01\x82\x01T`\xFF\x90\x81\x16` \x80\x84\x01\x91\x90\x91R`@\x80Q\x80\x82\x01\x82R`\x02\x86\x01T\x80\x85\x16\x82Ra\x01\0\x90\x04\x84\x16\x81\x84\x01R\x81\x85\x01R`\x03\x85\x01T\x90\x92\x16``\x84\x01R`\x04\x84\x01\x80T\x83Q\x81\x84\x02\x81\x01\x84\x01\x90\x94R\x80\x84R`\x80\x90\x94\x01\x93\x90\x91\x83\x01\x82\x82\x80\x15a\x07\x0CW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x06\xF8W[PPPPP\x81R` \x01`\x05\x82\x01\x80T\x80` \x02` \x01`@Q\x90\x81\x01`@R\x80\x92\x91\x90\x81\x81R` \x01\x82\x80T\x80\x15a\x07dW` \x02\x82\x01\x91\x90`\0R` `\0 \x90[\x81T\x81R` \x01\x90`\x01\x01\x90\x80\x83\x11a\x07PW[PPPPP\x81RPP\x90Pa\x07x\x81a\x08\xBFV[\x15a\x07\xB1W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\t`$\x82\x01Rh\x1A[\x9A\x1AX\x9A]\x19Y`\xBA\x1B`D\x82\x01R`d\x01a\x05>V[\x80` \x01Q`\xFF\x16\x83`\xFF\x16\x14a\x07\xCAWa\x07\xCAa\x10\x87V[`\0[`\0T`\xFF\x90\x81\x16\x90\x82\x16\x10\x15a\x07\xFBWa\x07\xE9\x81\x83\x85a\t\xF9V[\x80a\x07\xF3\x81a\x10\x9DV[\x91PPa\x07\xCDV[P`\x02\x80T\x90`\0a\x08\x0C\x83a\x10nV[\x91\x90PUP\x81\x83`\xFF\x16\x82``\x01Q`\xFF\x16\x7FP\xE4\xA5+\x07r\xBE\xD9\xF0j}?}\xFAf\xD76@\x06z\\\xC7zs\xC2EV\xCC\xC9\0\xFA\x08`\x02T`@Qa\x08Q\x91\x81R` \x01\x90V[`@Q\x80\x91\x03\x90\xA4PPPV[`\0[`\t\x81`\xFF\x16\x10\x15a\x08\xB9W`\x03a\x08z\x82`\x01a\x10\xBCV[`\xFF\x16`\n\x81\x10a\x08\x8DWa\x08\x8Da\x10 V[\x01T`\x03\x82`\xFF\x16`\n\x81\x10a\x08\xA5Wa\x08\xA5a\x10 V[\x01U\x80a\x08\xB1\x81a\x10\x9DV[\x91PPa\x08aV[P`\x0CUV[`\0\x80[`\x01`\xFF\x82\x16\x10\x15a\t\xF0W\x82`\xA0\x01Q\x81`\xFF\x16\x81Q\x81\x10a\x08\xE8Wa\x08\xE8a\x10 V[` \x02` \x01\x01Q`\0\x14a\t\xDEW`\0\x83`\xA0\x01Q\x82`\xFF\x16\x81Q\x81\x10a\t\x12Wa\t\x12a\x10 V[` \x02` \x01\x01Q\x12\x15a\t\x81W`\0\x83`\xA0\x01Q\x82`\xFF\x16\x81Q\x81\x10a\t;Wa\t;a\x10 V[` \x02` \x01\x01Q`\r\x83`\xFF\x16\x81T\x81\x10a\tYWa\tYa\x10 V[\x90`\0R` `\0 \x01Ta\tn\x91\x90a\x10\xDBV[\x12a\t|WP`\x01\x92\x91PPV[a\t\xDEV[`\0\x83`\xA0\x01Q\x82`\xFF\x16\x81Q\x81\x10a\t\x9CWa\t\x9Ca\x10 V[` \x02` \x01\x01Q`\r\x83`\xFF\x16\x81T\x81\x10a\t\xBAWa\t\xBAa\x10 V[\x90`\0R` `\0 \x01Ta\t\xCF\x91\x90a\x11\x03V[\x12\x15a\t\xDEWP`\x01\x92\x91PPV[\x80a\t\xE8\x81a\x10\x9DV[\x91PPa\x08\xC3V[P`\0\x92\x91PPV[`\0\x81\x11a\n:W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x0E`$\x82\x01Rm4\xB7;0\xB64\xB2\x109\xB1\xB0\xB60\xB9`\x91\x1B`D\x82\x01R`d\x01a\x05>V[\x81`\x80\x01Q\x83`\xFF\x16\x81Q\x81\x10a\nSWa\nSa\x10 V[` \x02` \x01\x01Q`\0\x14a\x0B\xEBW\x80\x82`\x80\x01Q\x84`\xFF\x16\x81Q\x81\x10a\n|Wa\n|a\x10 V[` \x02` \x01\x01Qa\n\x8E\x91\x90a\x11*V[`\r\x84`\xFF\x16\x81T\x81\x10a\n\xA4Wa\n\xA4a\x10 V[\x90`\0R` `\0 \x01Ta\n\xB9\x91\x90a\x10\xDBV[`\r\x84`\xFF\x16\x81T\x81\x10a\n\xCFWa\n\xCFa\x10 V[\x90`\0R` `\0 \x01\x81\x90UP`\0`\r\x84`\xFF\x16\x81T\x81\x10a\n\xF5Wa\n\xF5a\x10 V[\x90`\0R` `\0 \x01T\x12\x15a\x0B:W`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\t`$\x82\x01Rhunderflow`\xB8\x1B`D\x82\x01R`d\x01a\x05>V[`\0\x80\x84`\xFF\x16\x81T\x81\x10a\x0BQWa\x0BQa\x10 V[\x90`\0R` `\0 \x90`\x05\x02\x01`\x04\x01T\x11\x15a\x0B\xEBW`\0\x83`\xFF\x16\x81T\x81\x10a\x0B\x7FWa\x0B\x7Fa\x10 V[\x90`\0R` `\0 \x90`\x05\x02\x01`\x04\x01T`\r\x84`\xFF\x16\x81T\x81\x10a\x0B\xA7Wa\x0B\xA7a\x10 V[\x90`\0R` `\0 \x01T\x13\x15a\x0B\xEBW`@QbF\x1B\xCD`\xE5\x1B\x81R` `\x04\x82\x01R`\x08`$\x82\x01Rgoverflow`\xC0\x1B`D\x82\x01R`d\x01a\x05>V[PPPV[`@Q\x80`\xA0\x01`@R\x80a\x0C\x03a\x0C%V[\x81R` \x01`\0\x81R` \x01``\x81R` \x01``\x81R` \x01``\x81RP\x90V[`@Q\x80a\x01@\x01`@R\x80`\n\x90` \x82\x02\x806\x837P\x91\x92\x91PPV[`\0` \x82\x84\x03\x12\x15a\x0CVW`\0\x80\xFD[P5\x91\x90PV[`\0\x81Q\x80\x84R` \x80\x85\x01\x94P\x80\x84\x01`\0[\x83\x81\x10\x15a\x0C\x8DW\x81Q\x87R\x95\x82\x01\x95\x90\x82\x01\x90`\x01\x01a\x0CqV[P\x94\x95\x94PPPPPV[`\0\x81Q\x80\x84R`\0[\x81\x81\x10\x15a\x0C\xBEW` \x81\x85\x01\x81\x01Q\x86\x83\x01\x82\x01R\x01a\x0C\xA2V[P`\0` \x82\x86\x01\x01R` `\x1F\x19`\x1F\x83\x01\x16\x85\x01\x01\x91PP\x92\x91PPV[`\0\x81Q\x80\x84R` \x80\x85\x01\x80\x81\x96P\x83`\x05\x1B\x81\x01\x91P\x82\x86\x01`\0[\x85\x81\x10\x15a\r{W\x82\x84\x03\x89R\x81Q`\xC0\x81Q\x81\x87Ra\r\x1E\x82\x88\x01\x82a\x0C\x98V[\x91PP`\xFF\x87\x83\x01Q\x16\x87\x87\x01R`@\x80\x83\x01Qa\rN\x82\x89\x01\x82\x80Q`\xFF\x90\x81\x16\x83R` \x91\x82\x01Q\x16\x91\x01RV[PP``\x82\x01Q`\x80\x87\x81\x01\x91\x90\x91R\x90\x91\x01Q`\xA0\x90\x95\x01\x94\x90\x94R\x97\x84\x01\x97\x90\x84\x01\x90`\x01\x01a\x0C\xFCV[P\x91\x97\x96PPPPPPPV[`\0\x81Q\x80\x84R` \x80\x85\x01\x80\x81\x96P\x83`\x05\x1B\x81\x01\x91P\x82\x86\x01`\0[\x85\x81\x10\x15a\r{W\x82\x84\x03\x89R\x81Q`\xE0\x81Q\x81\x87Ra\r\xC8\x82\x88\x01\x82a\x0C\x98V[\x83\x89\x01Q`\xFF\x90\x81\x16\x89\x8B\x01R`@\x80\x86\x01Q\x80Q\x83\x16\x82\x8C\x01R` \x81\x01Q\x83\x16``\x8C\x01R\x92\x94P\x92P\x90P``\x84\x01Q\x91P`\x80\x81\x83\x16\x81\x8A\x01R\x80\x85\x01Q\x92PPP`\xA0\x87\x83\x03\x81\x89\x01Ra\x0E!\x83\x83a\x0C]V[\x93\x01Q\x87\x84\x03`\xC0\x89\x01R\x92\x91Pa\x0E;\x90P\x81\x83a\x0C]V[\x9A\x87\x01\x9A\x95PPP\x90\x84\x01\x90`\x01\x01a\r\xA6V[` \x80\x82R\x82Q`\0\x91\x90\x82\x84\x83\x01[`\n\x82\x10\x15a\x0E~W\x82Q\x81R\x91\x83\x01\x91`\x01\x91\x90\x91\x01\x90\x83\x01a\x0E_V[PPP\x83\x01Qa\x01`\x83\x01R`@\x83\x01Qa\x01\xC0a\x01\x80\x84\x01\x81\x90Ra\x0E\xA8a\x01\xE0\x85\x01\x83a\x0C]V[\x91P``\x85\x01Q`\x1F\x19\x80\x86\x85\x03\x01a\x01\xA0\x87\x01Ra\x0E\xC7\x84\x83a\x0C\xDEV[\x93P`\x80\x87\x01Q\x91P\x80\x86\x85\x03\x01\x83\x87\x01RPa\x0E\xE4\x83\x82a\r\x88V[\x96\x95PPPPPPV[\x805`\xFF\x81\x16\x81\x14a\x0E\xFFW`\0\x80\xFD[\x91\x90PV[`\0\x80`@\x83\x85\x03\x12\x15a\x0F\x17W`\0\x80\xFD[a\x0F \x83a\x0E\xEEV[\x94` \x93\x90\x93\x015\x93PPPV[`\0\x80\x83`\x1F\x84\x01\x12a\x0F@W`\0\x80\xFD[P\x815g\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x81\x11\x15a\x0FXW`\0\x80\xFD[` \x83\x01\x91P\x83` \x82`\x05\x1B\x85\x01\x01\x11\x15a\x0FsW`\0\x80\xFD[\x92P\x92\x90PV[`\0\x80`\0\x80`@\x85\x87\x03\x12\x15a\x0F\x90W`\0\x80\xFD[\x845g\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x80\x82\x11\x15a\x0F\xA8W`\0\x80\xFD[a\x0F\xB4\x88\x83\x89\x01a\x0F.V[\x90\x96P\x94P` \x87\x015\x91P\x80\x82\x11\x15a\x0F\xCDW`\0\x80\xFD[Pa\x0F\xDA\x87\x82\x88\x01a\x0F.V[\x95\x98\x94\x97P\x95PPPPV[`\x01\x81\x81\x1C\x90\x82\x16\x80a\x0F\xFAW`\x7F\x82\x16\x91P[` \x82\x10\x81\x03a\x10\x1AWcNH{q`\xE0\x1B`\0R`\"`\x04R`$`\0\xFD[P\x91\x90PV[cNH{q`\xE0\x1B`\0R`2`\x04R`$`\0\xFD[`\0` \x82\x84\x03\x12\x15a\x10HW`\0\x80\xFD[a\x10Q\x82a\x0E\xEEV[\x93\x92PPPV[cNH{q`\xE0\x1B`\0R`\x11`\x04R`$`\0\xFD[`\0`\x01\x82\x01a\x10\x80Wa\x10\x80a\x10XV[P`\x01\x01\x90V[cNH{q`\xE0\x1B`\0R`\x01`\x04R`$`\0\xFD[`\0`\xFF\x82\x16`\xFF\x81\x03a\x10\xB3Wa\x10\xB3a\x10XV[`\x01\x01\x92\x91PPV[`\xFF\x81\x81\x16\x83\x82\x16\x01\x90\x81\x11\x15a\x10\xD5Wa\x10\xD5a\x10XV[\x92\x91PPV[\x80\x82\x01\x82\x81\x12`\0\x83\x12\x80\x15\x82\x16\x82\x15\x82\x16\x17\x15a\x10\xFBWa\x10\xFBa\x10XV[PP\x92\x91PPV[\x81\x81\x03`\0\x83\x12\x80\x15\x83\x83\x13\x16\x83\x83\x12\x82\x16\x17\x15a\x11#Wa\x11#a\x10XV[P\x92\x91PPV[\x80\x82\x02`\0\x82\x12`\x01`\xFF\x1B\x84\x14\x16\x15a\x11FWa\x11Fa\x10XV[\x81\x81\x05\x83\x14\x82\x15\x17a\x10\xD5Wa\x10\xD5a\x10XV\xFE\xA2dipfsX\"\x12 C\xBA\x8BiP\xA9\xCDh\xD9h=\xA7\xA0\x08O(\xF8tvI#\xF9\r\xD4\xF7\xCF\x01}\x08MG\x94dsolcC\0\x08\x15\x003";
    /// The deployed bytecode of the contract.
    pub static MYSTATEMACHINE_DEPLOYED_BYTECODE: ::ethers::core::types::Bytes = ::ethers::core::types::Bytes::from_static(
        __DEPLOYED_BYTECODE,
    );
    pub struct MyStateMachine<M>(::ethers::contract::Contract<M>);
    impl<M> ::core::clone::Clone for MyStateMachine<M> {
        fn clone(&self) -> Self {
            Self(::core::clone::Clone::clone(&self.0))
        }
    }
    impl<M> ::core::ops::Deref for MyStateMachine<M> {
        type Target = ::ethers::contract::Contract<M>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl<M> ::core::ops::DerefMut for MyStateMachine<M> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
    impl<M> ::core::fmt::Debug for MyStateMachine<M> {
        fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            f.debug_tuple(::core::stringify!(MyStateMachine))
                .field(&self.address())
                .finish()
        }
    }
    impl<M: ::ethers::providers::Middleware> MyStateMachine<M> {
        /// Creates a new contract instance with the specified `ethers` client at
        /// `address`. The contract derefs to a `ethers::Contract` object.
        pub fn new<T: Into<::ethers::core::types::Address>>(
            address: T,
            client: ::std::sync::Arc<M>,
        ) -> Self {
            Self(
                ::ethers::contract::Contract::new(
                    address.into(),
                    MYSTATEMACHINE_ABI.clone(),
                    client,
                ),
            )
        }
        /// Constructs the general purpose `Deployer` instance based on the provided constructor arguments and sends it.
        /// Returns a new instance of a deployer that returns an instance of this contract after sending the transaction
        ///
        /// Notes:
        /// - If there are no constructor arguments, you should pass `()` as the argument.
        /// - The default poll duration is 7 seconds.
        /// - The default number of confirmations is 1 block.
        ///
        ///
        /// # Example
        ///
        /// Generate contract bindings with `abigen!` and deploy a new contract instance.
        ///
        /// *Note*: this requires a `bytecode` and `abi` object in the `greeter.json` artifact.
        ///
        /// ```ignore
        /// # async fn deploy<M: ethers::providers::Middleware>(client: ::std::sync::Arc<M>) {
        ///     abigen!(Greeter, "../greeter.json");
        ///
        ///    let greeter_contract = Greeter::deploy(client, "Hello world!".to_string()).unwrap().send().await.unwrap();
        ///    let msg = greeter_contract.greet().call().await.unwrap();
        /// # }
        /// ```
        pub fn deploy<T: ::ethers::core::abi::Tokenize>(
            client: ::std::sync::Arc<M>,
            constructor_args: T,
        ) -> ::core::result::Result<
            ::ethers::contract::builders::ContractDeployer<M, Self>,
            ::ethers::contract::ContractError<M>,
        > {
            let factory = ::ethers::contract::ContractFactory::new(
                MYSTATEMACHINE_ABI.clone(),
                MYSTATEMACHINE_BYTECODE.clone().into(),
                client,
            );
            let deployer = factory.deploy(constructor_args)?;
            let deployer = ::ethers::contract::ContractDeployer::new(deployer);
            Ok(deployer)
        }
        ///Calls the contract's `context` (0xd0496d6a) function
        pub fn context(&self) -> ::ethers::contract::builders::ContractCall<M, Head> {
            self.0
                .method_hash([208, 73, 109, 106], ())
                .expect("method not found (this should never happen)")
        }
        ///Calls the contract's `latestBlocks` (0x6ee376e6) function
        pub fn latest_blocks(
            &self,
            p0: ::ethers::core::types::U256,
        ) -> ::ethers::contract::builders::ContractCall<M, ::ethers::core::types::U256> {
            self.0
                .method_hash([110, 227, 118, 230], p0)
                .expect("method not found (this should never happen)")
        }
        ///Calls the contract's `sequence` (0x529d15cc) function
        pub fn sequence(
            &self,
        ) -> ::ethers::contract::builders::ContractCall<M, ::ethers::core::types::U256> {
            self.0
                .method_hash([82, 157, 21, 204], ())
                .expect("method not found (this should never happen)")
        }
        ///Calls the contract's `signal` (0xddc3b187) function
        pub fn signal(
            &self,
            action: u8,
            scalar: ::ethers::core::types::U256,
        ) -> ::ethers::contract::builders::ContractCall<M, ()> {
            self.0
                .method_hash([221, 195, 177, 135], (action, scalar))
                .expect("method not found (this should never happen)")
        }
        ///Calls the contract's `signalMany` (0xfff01fe2) function
        pub fn signal_many(
            &self,
            actions: ::std::vec::Vec<u8>,
            scalars: ::std::vec::Vec<::ethers::core::types::U256>,
        ) -> ::ethers::contract::builders::ContractCall<M, ()> {
            self.0
                .method_hash([255, 240, 31, 226], (actions, scalars))
                .expect("method not found (this should never happen)")
        }
        ///Calls the contract's `state` (0x3e4f49e6) function
        pub fn state(
            &self,
            p0: ::ethers::core::types::U256,
        ) -> ::ethers::contract::builders::ContractCall<M, ::ethers::core::types::I256> {
            self.0
                .method_hash([62, 79, 73, 230], p0)
                .expect("method not found (this should never happen)")
        }
        ///Gets the contract's `SignaledEvent` event
        pub fn signaled_event_filter(
            &self,
        ) -> ::ethers::contract::builders::Event<
            ::std::sync::Arc<M>,
            M,
            SignaledEventFilter,
        > {
            self.0.event()
        }
        /// Returns an `Event` builder for all the events of this contract.
        pub fn events(
            &self,
        ) -> ::ethers::contract::builders::Event<
            ::std::sync::Arc<M>,
            M,
            SignaledEventFilter,
        > {
            self.0.event_with_filter(::core::default::Default::default())
        }
    }
    impl<M: ::ethers::providers::Middleware> From<::ethers::contract::Contract<M>>
    for MyStateMachine<M> {
        fn from(contract: ::ethers::contract::Contract<M>) -> Self {
            Self::new(contract.address(), contract.client())
        }
    }
    #[derive(
        Clone,
        ::ethers::contract::EthEvent,
        ::ethers::contract::EthDisplay,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethevent(
        name = "SignaledEvent",
        abi = "SignaledEvent(uint8,uint8,uint256,uint256)"
    )]
    pub struct SignaledEventFilter {
        #[ethevent(indexed)]
        pub role: u8,
        #[ethevent(indexed)]
        pub action_id: u8,
        #[ethevent(indexed)]
        pub scalar: ::ethers::core::types::U256,
        pub sequence: ::ethers::core::types::U256,
    }
    ///Container type for all input parameters for the `context` function with signature `context()` and selector `0xd0496d6a`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "context", abi = "context()")]
    pub struct ContextCall;
    ///Container type for all input parameters for the `latestBlocks` function with signature `latestBlocks(uint256)` and selector `0x6ee376e6`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "latestBlocks", abi = "latestBlocks(uint256)")]
    pub struct LatestBlocksCall(pub ::ethers::core::types::U256);
    ///Container type for all input parameters for the `sequence` function with signature `sequence()` and selector `0x529d15cc`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "sequence", abi = "sequence()")]
    pub struct SequenceCall;
    ///Container type for all input parameters for the `signal` function with signature `signal(uint8,uint256)` and selector `0xddc3b187`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "signal", abi = "signal(uint8,uint256)")]
    pub struct SignalCall {
        pub action: u8,
        pub scalar: ::ethers::core::types::U256,
    }
    ///Container type for all input parameters for the `signalMany` function with signature `signalMany(uint8[],uint256[])` and selector `0xfff01fe2`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "signalMany", abi = "signalMany(uint8[],uint256[])")]
    pub struct SignalManyCall {
        pub actions: ::std::vec::Vec<u8>,
        pub scalars: ::std::vec::Vec<::ethers::core::types::U256>,
    }
    ///Container type for all input parameters for the `state` function with signature `state(uint256)` and selector `0x3e4f49e6`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "state", abi = "state(uint256)")]
    pub struct StateCall(pub ::ethers::core::types::U256);
    ///Container type for all of the contract's call
    #[derive(
        Clone,
        ::ethers::contract::EthAbiType,
        serde::Serialize,
        serde::Deserialize,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    pub enum MyStateMachineCalls {
        Context(ContextCall),
        LatestBlocks(LatestBlocksCall),
        Sequence(SequenceCall),
        Signal(SignalCall),
        SignalMany(SignalManyCall),
        State(StateCall),
    }
    impl ::ethers::core::abi::AbiDecode for MyStateMachineCalls {
        fn decode(
            data: impl AsRef<[u8]>,
        ) -> ::core::result::Result<Self, ::ethers::core::abi::AbiError> {
            let data = data.as_ref();
            if let Ok(decoded) = <ContextCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::Context(decoded));
            }
            if let Ok(decoded) = <LatestBlocksCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::LatestBlocks(decoded));
            }
            if let Ok(decoded) = <SequenceCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::Sequence(decoded));
            }
            if let Ok(decoded) = <SignalCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::Signal(decoded));
            }
            if let Ok(decoded) = <SignalManyCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::SignalMany(decoded));
            }
            if let Ok(decoded) = <StateCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::State(decoded));
            }
            Err(::ethers::core::abi::Error::InvalidData.into())
        }
    }
    impl ::ethers::core::abi::AbiEncode for MyStateMachineCalls {
        fn encode(self) -> Vec<u8> {
            match self {
                Self::Context(element) => ::ethers::core::abi::AbiEncode::encode(element),
                Self::LatestBlocks(element) => {
                    ::ethers::core::abi::AbiEncode::encode(element)
                }
                Self::Sequence(element) => {
                    ::ethers::core::abi::AbiEncode::encode(element)
                }
                Self::Signal(element) => ::ethers::core::abi::AbiEncode::encode(element),
                Self::SignalMany(element) => {
                    ::ethers::core::abi::AbiEncode::encode(element)
                }
                Self::State(element) => ::ethers::core::abi::AbiEncode::encode(element),
            }
        }
    }
    impl ::core::fmt::Display for MyStateMachineCalls {
        fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            match self {
                Self::Context(element) => ::core::fmt::Display::fmt(element, f),
                Self::LatestBlocks(element) => ::core::fmt::Display::fmt(element, f),
                Self::Sequence(element) => ::core::fmt::Display::fmt(element, f),
                Self::Signal(element) => ::core::fmt::Display::fmt(element, f),
                Self::SignalMany(element) => ::core::fmt::Display::fmt(element, f),
                Self::State(element) => ::core::fmt::Display::fmt(element, f),
            }
        }
    }
    impl ::core::convert::From<ContextCall> for MyStateMachineCalls {
        fn from(value: ContextCall) -> Self {
            Self::Context(value)
        }
    }
    impl ::core::convert::From<LatestBlocksCall> for MyStateMachineCalls {
        fn from(value: LatestBlocksCall) -> Self {
            Self::LatestBlocks(value)
        }
    }
    impl ::core::convert::From<SequenceCall> for MyStateMachineCalls {
        fn from(value: SequenceCall) -> Self {
            Self::Sequence(value)
        }
    }
    impl ::core::convert::From<SignalCall> for MyStateMachineCalls {
        fn from(value: SignalCall) -> Self {
            Self::Signal(value)
        }
    }
    impl ::core::convert::From<SignalManyCall> for MyStateMachineCalls {
        fn from(value: SignalManyCall) -> Self {
            Self::SignalMany(value)
        }
    }
    impl ::core::convert::From<StateCall> for MyStateMachineCalls {
        fn from(value: StateCall) -> Self {
            Self::State(value)
        }
    }
    ///Container type for all return fields from the `context` function with signature `context()` and selector `0xd0496d6a`
    #[derive(
        Clone,
        ::ethers::contract::EthAbiType,
        ::ethers::contract::EthAbiCodec,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    pub struct ContextReturn(pub Head);
    ///Container type for all return fields from the `latestBlocks` function with signature `latestBlocks(uint256)` and selector `0x6ee376e6`
    #[derive(
        Clone,
        ::ethers::contract::EthAbiType,
        ::ethers::contract::EthAbiCodec,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    pub struct LatestBlocksReturn(pub ::ethers::core::types::U256);
    ///Container type for all return fields from the `sequence` function with signature `sequence()` and selector `0x529d15cc`
    #[derive(
        Clone,
        ::ethers::contract::EthAbiType,
        ::ethers::contract::EthAbiCodec,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    pub struct SequenceReturn(pub ::ethers::core::types::U256);
    ///Container type for all return fields from the `state` function with signature `state(uint256)` and selector `0x3e4f49e6`
    #[derive(
        Clone,
        ::ethers::contract::EthAbiType,
        ::ethers::contract::EthAbiCodec,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    pub struct StateReturn(pub ::ethers::core::types::I256);
}
