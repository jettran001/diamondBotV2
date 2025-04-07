# Diamond Mining Module

Smart contract cho mining và phát thưởng Diamond Token (DMD) trên NEAR Protocol.

## Tính năng

- Mining tokens dựa trên số block đã qua
- Tự động tính và cập nhật phần thưởng
- Quản lý linh hoạt với tỷ lệ phần thưởng có thể điều chỉnh
- Tích hợp trực tiếp với Diamond Token

## Triển khai

```bash
near deploy --wasmFile target/wasm32-unknown-unknown/release/diamond_mining.wasm --accountId <mining_account_id>

# Khởi tạo contract
near call <mining_account_id> new '{"diamond_token": "<diamond_token_id>"}' --accountId <owner_account_id>
```

## Sử dụng

```bash
# Cập nhật phần thưởng
near call <mining_account_id> update_rewards '{}' --accountId <your_account_id>

# Xem phần thưởng hiện tại
near view <mining_account_id> get_pending_reward '{"account_id": "<your_account_id>"}'

# Claim phần thưởng
near call <mining_account_id> claim_rewards '{}' --accountId <your_account_id> --depositYocto 1 --gas 100000000000000
```

## Quản lý (Owner)

```bash
# Đặt tỷ lệ phần thưởng mới
near call <mining_account_id> set_reward_rate '{"reward_rate": "20000000000000000000"}' --accountId <owner_account_id> --depositYocto 1

# Cập nhật block phần thưởng toàn cục
near call <mining_account_id> force_update_global_reward '{}' --accountId <owner_account_id> --depositYocto 1
```
```

## 5. Cập nhật Cargo.toml trong workspace

### Đường dẫn: `blockchain/Cargo.toml`

Workspace đã có cấu hình đúng, vì đã bao gồm các thư mục con trong thư mục src, nhưng hãy kiểm tra lại:

```toml
[workspace]
members = [
    "src",
    "src/diamond_token",
    "src/farming",
    "src/mining"
]
```

## 6. Tích hợp vào Diamond Token

Sau khi triển khai Diamond Mining, bạn cần đăng ký nó với Diamond Token thông qua Module Manager:

```bash
# Thêm module mining vào Diamond Token
near call <diamond_token_id> add_module '{
  "module": {
    "name": "mining",
    "contract_id": "<mining_account_id>",
    "version": "1.0.0",
    "status": "Active",
    "added_timestamp": 0,
    "last_updated": 0,
    "metadata": "{\"description\":\"Diamond Token Mining Module\",\"reward_rate\":\"10 DMD per block\"}"
  }
}' --accountId <owner_account_id> --depositYocto 1

# Cấp quyền minting cho Mining contract
near call <diamond_token_id> authorize_caller '{
  "contract_id": "<mining_account_id>",
  "permissions": ["mint", "ft_balance_of"]
}' --accountId <owner_account_id> --depositYocto 1
```

## Biên dịch và triển khai

```bash
# Di chuyển đến thư mục mining
cd blockchain/src/mining

# Biên dịch
cargo build --target wasm32-unknown-unknown --release

# Triển khai
near deploy --wasmFile target/wasm32-unknown-unknown/release/diamond_mining.wasm --accountId <mining_account_id>

# Khởi tạo contract
near call <mining_account_id> new '{"diamond_token": "<diamond_token_id>"}' --accountId <owner_account_id>
```

Với những bước trên, module DiamondMining đã được tích hợp vào hệ thống Diamond Token. Module này cho phép người dùng nhận phần thưởng DMD dựa trên số block đã trôi qua, được triển khai tương đương với code Solidity bạn cung cấp.