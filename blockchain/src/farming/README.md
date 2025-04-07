# Diamond Farming Module

Smart contract cho staking và farming Diamond Token (DMD) trên NEAR Protocol.

## Tính năng

- Staking DMD token với thời gian khóa 30 ngày
- Hệ thống phần thưởng tỷ lệ với số lượng token staked
- Quản lý phần thưởng linh hoạt với tỷ lệ phần thưởng có thể điều chỉnh
- Tích hợp đầy đủ với Diamond Token

## Triển khai

```bash
near deploy --wasmFile target/wasm32-unknown-unknown/release/diamond_farming.wasm --accountId <farming_account_id>

# Khởi tạo contract
near call <farming_account_id> new '{"staking_token": "<diamond_token_id>"}' --accountId <owner_account_id>
```

## Sử dụng

```bash
# Stake token
near call <diamond_token_id> ft_transfer_call '{
  "receiver_id": "<farming_account_id>",
  "amount": "1000000000000000000000000", 
  "msg": "{\"action\":\"stake\",\"user_id\":\"<your_account_id>\"}"
}' --accountId <your_account_id> --depositYocto 1 --gas 100000000000000

# Xem stake hiện tại
near view <farming_account_id> get_stake '{"account_id": "<your_account_id>"}'

# Claim phần thưởng
near call <farming_account_id> claim_rewards '{}' --accountId <your_account_id> --depositYocto 1
```
