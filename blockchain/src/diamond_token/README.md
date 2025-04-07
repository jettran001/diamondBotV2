# Diamond Token (DMD)

Diamond Token là một token đa chuỗi (omni token) được khởi tạo trên NEAR Protocol với cơ chế MPC (Multi-Party Computation) và sử dụng LayerZero để kết nối liền mạch giữa các blockchain khác nhau.

## Tính năng chính

- **Tiêu chuẩn NEP-141**: Tuân thủ chuẩn token NEAR (NEP-141)
- **Đa chuỗi**: Chuyển token liền mạch giữa các blockchain qua LayerZero
- **Bảo mật MPC**: Sử dụng cơ chế đa chữ ký cho giao dịch cross-chain
- **Tổng cung giới hạn**: 1 tỷ DMD tokens, khởi tạo 250 triệu
- **Quản lý tổng cung toàn cầu**: Theo dõi và quản lý tổng cung trên tất cả các chain
- **Kiến trúc mở rộng**: Hỗ trợ thêm các module chức năng mới theo nhu cầu

## Cấu trúc Smart Contract

Dự án bao gồm các thành phần chính:
1. **DiamondToken**: Smart contract NEP-141 core
2. **MPCController**: Cơ chế đa chữ ký cho bridge an toàn
3. **SupplyTracker**: Theo dõi tổng cung token trên các chain
4. **ModuleManager**: Quản lý các module có thể mở rộng

## Triển khai và sử dụng

### Biên dịch và triển khai

```bash
# Biên dịch
cargo build --target wasm32-unknown-unknown --release

# Triển khai trên NEAR testnet
near deploy --wasmFile target/wasm32-unknown-unknown/release/diamond_token.wasm --accountId <your_account_id>
```

### Khởi tạo contract

```bash
near call <contract_id> new '{
  "owner_id": "<owner_account_id>", 
  "lz_endpoint": "<lz_endpoint_account_id>",
  "near_chain_id": 1313161554
}' --accountId <your_account_id>
```

### Quản lý token

```bash
# Chuyển token
near call <contract_id> ft_transfer '{
  "receiver_id": "<receiver_account_id>", 
  "amount": "1000000000000000000000000", 
  "memo": "Chuyển token"
}' --accountId <your_account_id> --depositYocto 1

# Đốt token
near call <contract_id> burn '{
  "amount": "1000000000000000000000000"
}' --accountId <your_account_id> --depositYocto 1

# Mint token (chỉ owner)
near call <contract_id> mint '{
  "account_id": "<receiver_account_id>", 
  "amount": "1000000000000000000000000"
}' --accountId <owner_account_id> --depositYocto 1
```

### Cấu hình Bridge

```bash
# Thiết lập cấu hình bridge cho một blockchain
near call <contract_id> set_chain_config '{
  "chain_id": 1,
  "config": {
    "remote_chain_id": 1,
    "remote_bridge_address": "0x1234567890123456789012345678901234567890",
    "chain_name": "Ethereum",
    "enabled": true,
    "fee_basis_points": 50,
    "min_fee": "1000000000000000000",
    "max_fee": "100000000000000000000",
    "supply_oracle": null
  }
}' --accountId <owner_account_id> --depositYocto 1

# Thiết lập oracle để cập nhật supply
near call <contract_id> set_supply_oracle '{
  "chain_id": 1,
  "oracle": "oracle.near"
}' --accountId <owner_account_id> --depositYocto 1

# Lấy thông tin cấu hình bridge
near view <contract_id> get_chain_config '{"chain_id": 1}'
```

### Sử dụng Bridge

```bash
# Gửi token từ NEAR đến blockchain khác
near call <contract_id> bridge_out '{
  "to": "0xReceiverAddressOnDestChain",
  "amount": "1000000000000000000000",
  "lz_params": {
    "dest_chain_id": 1,
    "adapter_params": [],
    "fees": "10000000000000000000"
  }
}' --accountId <your_account_id> --depositYocto 1
```

### Quản lý Module Mở Rộng

```bash
# Thêm một module mới
near call <contract_id> add_module '{
  "module": {
    "name": "staking",
    "contract_id": "staking.near",
    "version": "1.0.0",
    "status": "Active",
    "added_timestamp": 0,
    "last_updated": 0,
    "metadata": "{\"description\":\"Diamond Staking Module\"}"
  }
}' --accountId <owner_account_id> --depositYocto 1

# Lấy tất cả module đang hoạt động
near view <contract_id> get_active_modules '{}'

# Gọi hàm từ module
near call <contract_id> call_module_function '{
  "module_name": "staking",
  "function_name": "stake",
  "args": "{\"amount\":\"1000000000000000000000000\"}"
}' --accountId <owner_account_id> --depositYocto 1
```

### Quản lý Tổng Cung Toàn Cầu

```bash
# Lấy tổng cung trên tất cả các chain
near view <contract_id> get_global_circulating_supply '{}'

# Lấy tổng cung trên NEAR
near view <contract_id> get_near_circulating_supply '{}'

# Lấy tổng cung trên các chain khác nhau
near view <contract_id> get_chain_supplies '{}'

# Cập nhật tổng cung từ chain khác (yêu cầu oracle)
near call <contract_id> update_remote_chain_supply '{
  "chain_id": 1,
  "supply": "50000000000000000000000000"
}' --accountId <oracle_account_id> --depositYocto 1
```

## Quản trị và Mở rộng

### Cách thêm Module Mới

Diamond Token hỗ trợ kiến trúc mở rộng, cho phép thêm các module mới mà không cần thay đổi smart contract chính:

1. Triển khai module mới dưới dạng một smart contract riêng biệt
2. Thêm module vào Module Manager thông qua hàm `add_module`
3. Cấu hình quyền và các tương tác giữa module và Diamond Token

### Kiểm soát Admin

```bash
# Thêm admin mới cho module manager
near call <contract_id> add_module_admin '{
  "admin_id": "new_admin.near"
}' --accountId <owner_account_id> --depositYocto 1

# Chuyển quyền sở hữu
near call <contract_id> transfer_ownership '{
  "new_owner_id": "new_owner.near"
}' --accountId <owner_account_id> --depositYocto 1
```

## Tính năng MPC

MPC Controller đảm bảo an toàn cho giao dịch cross-chain bằng cách yêu cầu nhiều chữ ký từ các signer được ủy quyền:

```bash
# Thêm người ký được ủy quyền
near call <mpc_controller_id> add_authorized_signer '{
  "signer_id": "signer1.near"
}' --accountId <owner_account_id> --depositYocto 1

# Thiết lập threshold (số chữ ký tối thiểu)
near call <mpc_controller_id> set_threshold '{
  "threshold": 3
}' --accountId <owner_account_id> --depositYocto 1
```

## Lưu ý bảo mật và thiết kế

- Kiến trúc mô-đun cho phép hệ thống phát triển mà không cần thay đổi contract chính
- Tổng cung được quản lý trên toàn bộ hệ sinh thái, đảm bảo tổng cung toàn cầu không vượt quá giới hạn
- Sử dụng oracle để cập nhật tổng cung từ các chain khác
- Mô hình đa chữ ký (MPC) đảm bảo an toàn cho giao dịch cross-chain
- Cơ chế kiểm soát quyền admin linh hoạt

## License

MIT License 