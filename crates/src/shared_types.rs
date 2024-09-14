///`Head(uint256[10],uint256,int256[],(string,uint8,(uint8,uint8),uint256,uint256)[],(string,uint8,(uint8,uint8),uint8,int256[],int256[])[])`
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
pub struct Head {
    pub latest_blocks: [::ethers::core::types::U256; 10],
    pub sequence: ::ethers::core::types::U256,
    pub state: ::std::vec::Vec<::ethers::core::types::I256>,
    pub places: ::std::vec::Vec<Place>,
    pub transitions: ::std::vec::Vec<Transition>,
}
///`Place(string,uint8,(uint8,uint8),uint256,uint256)`
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
pub struct Place {
    pub label: ::std::string::String,
    pub offset: u8,
    pub position: Position,
    pub initial: ::ethers::core::types::U256,
    pub capacity: ::ethers::core::types::U256,
}
///`Position(uint8,uint8)`
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
pub struct Position {
    pub x: u8,
    pub y: u8,
}
///`Transition(string,uint8,(uint8,uint8),uint8,int256[],int256[])`
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
pub struct Transition {
    pub label: ::std::string::String,
    pub offset: u8,
    pub position: Position,
    pub role: u8,
    pub delta: ::std::vec::Vec<::ethers::core::types::I256>,
    pub guard: ::std::vec::Vec<::ethers::core::types::I256>,
}
