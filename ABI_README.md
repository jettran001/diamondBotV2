# Hướng dẫn sử dụng ABI trong Diamond Chain

Để tránh trùng lặp và tăng tính tái sử dụng, tất cả các file ABI đã được chuyển từ `blockchain/abi` sang `blockchain/src/abi`. Codebase hiện đã được cập nhật một phần để sử dụng module mới này.

## Sử dụng ABI từ module chung

Thay vì sử dụng các import trực tiếp như trước đây:

```rust
// Cũ - KHÔNG SỬ DỤNG
let token_abi = serde_json::from_str(include_str!("../abi/erc20.json"))?;
```

Hãy sử dụng module `abi_utils` trong thư mục `snipebot`:

```rust
// Mới - NÊN SỬ DỤNG
use crate::abi_utils;

// Lấy ABI dưới dạng chuỗi JSON
let token_abi = abi_utils::get_erc20_abi();
let token_abi: ethers::abi::Abi = serde_json::from_str(token_abi)?;

// HOẶC sử dụng trực tiếp từ diamond_blockchain
use diamond_blockchain::abi;
let erc20_abi = &abi::abis::erc20::ERC20_ABI;
```

## Các ABI có sẵn

Module `abi_utils` cung cấp các hàm sau để lấy ABI:

- `get_erc20_abi()` - ABI cho token ERC20
- `get_router_abi()` - ABI cho Uniswap V2 Router
- `get_factory_abi()` - ABI cho Uniswap V2 Factory
- `get_pair_abi()` - ABI cho Uniswap V2 Pair

## Những file cần cập nhật

Danh sách các file chưa được cập nhật hoàn toàn để sử dụng module `abi_utils` mới:

1. `snipebot/src/chain_adapters/wallet_integration.rs`
2. `snipebot/src/chain_adapters/chain_adapter_impl.rs`
3. `snipebot/src/chain_adapters/ethereum.rs`
4. `snipebot/src/chain_adapters/bsc.rs`
5. `snipebot/src/chain_adapters/avalanche.rs` 
6. `snipebot/src/chain_adapters/monad.rs`
7. `snipebot/src/risk_analyzer.rs`
8. `snipebot/src/snipebot.rs`
9. `snipebot/src/blockchain.rs`

Khi cập nhật các file này, hãy thay thế các dòng:

```rust
include_str!("../../abi/erc20.json")
```

thành

```rust
abi_utils::get_erc20_abi()
```

và tương tự cho các ABI khác. 