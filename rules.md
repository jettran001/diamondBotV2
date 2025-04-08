### 📜 Tổng hợp quy tắc từ `.cursorrc`

#### 1. 📌 Nguyên tắc review code
- Mọi thay đổi **bắt buộc** tuân thủ thứ tự import, style guide, nguyên tắc module.
- **Cấm** sửa format, import, hoặc cấu trúc nếu không nằm trong mục đích của PR/commit.
- **Không được** đổi tên hàm/module/field **nếu không** có lý do rõ ràng và được duyệt.
- Phải **giải thích lý do** nếu dùng unwrap, panic, hoặc unsafe code.
- Tư duy hướng module: chia theo chức năng, **tránh lặp lại xử lý ở nhiều nơi**.

#### 2. 🧠 Patterns (Mẫu sử dụng)
- Sử dụng trait/enum để mô hình hóa hành vi phức tạp.
- Code liên quan đến logic giao dịch luôn phải qua tầng `TradeLogic`.
- Không sử dụng closure phức tạp trong async block trừ khi không còn cách khác.
- Tránh hardcode. Dùng config/constants nơi phù hợp.

#### 3. 🛡️ Enforcement (Thực thi)
```json
"enforcement": {
  "pre_condition": "BẮT BUỘC đọc và tuân thủ trước khi thực hiện bất kỳ thay đổi nào",
  "violation_consequence": "Không được phép thực hiện thay đổi nếu vi phạm nguyên tắc",
  "strict_violation": "BẮT BUỘC dừng ngay lập tức nếu phát hiện vi phạm quy tắc"
}
```

#### 4. 🧩 Structure & Module
- Các file phải nằm trong đúng module, theo chức năng: `api`, `tradelogic`, `snipebot`, `common`.
- **Cấm gọi chéo trực tiếp giữa các module** nếu không có interface rõ ràng.
- Layer `api` chỉ nên chứa logic về routing, không xử lý nghiệp vụ.
- Layer `tradelogic` là trung tâm xử lý logic → không bị phụ thuộc ngược bởi `api`.

#### 5. 🔄 Workflow phát triển
- Tất cả commit phải được format bằng `cargo fmt`.
- Code mới phải đi kèm test nếu chạm vào core logic.
- Mọi refactor lớn phải được ghi chú rõ mục đích và kết quả mong đợi.
