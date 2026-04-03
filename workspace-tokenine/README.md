# Tokenine Facebook Bot Workspace (ZeroClaw)

Workspace สำหรับ Facebook Page Bot ที่ใช้ตอบลูกค้าและลงทะเบียนคูปอง
สร้างบน **ZeroClaw** (Rust) ซึ่งมีประสิทธิภาพสูงกว่า PicoClaw

## 📁 โครงสร้างไฟล์

```
workspace-tokenine/
├── AGENT.md              # Main agent definition (โทนี่)
├── SOUL.md               # บุคลิกและสไตล์การสื่อสาร
├── USER.md               # ข้อมูลร้านและการตั้งค่า
├── config.example.toml   # ตัวอย่าง config
├── README.md             # เอกสารนี้
└── agents/
    └── coupon_validator.md   # Sub-agent สำหรับตรวจสอบคูปอง

src/hooks/builtin/
└── coupon_guardrail.rs   # Guardrail hook (ใน zeroclaw source)
```

## 🛡️ ระบบ Guardrail (การป้องกัน)

### 1. แยก Sub-Agent (หลัก)

```
┌─────────────────┐     ┌──────────────────┐
│  Main Agent     │────▶│  Coupon Validator │
│  (โทนี่)         │     │  (delegate agent)  │
└─────────────────┘     └──────────────────┘
        │                        │
        │                        ▼
        │                 ┌──────────────────┐
        │                 │  Coupon API      │
        │                 │  (ตรวจสอบ code)  │
        │                 └──────────────────┘
        │
        ▼
  ตอบลูกค้า
```

**หลักการ:**
- Main Agent **ไม่มีสิทธิ์** ลงทะเบียนคูปองโดยตรง
- ต้องส่งให้ `coupon_validator` agent ผ่าน `delegate` tool
- Hook ตรวจสอบทุกครั้งก่อน execute

### 2. CouponGuardrailHook

Hook จะ intercept `before_tool_call` และตรวจสอบ:

```rust
// 1. Check if tool is guarded (apply_coupon, checkout, etc.)
if !guarded_tools.contains(&name) { return Continue; }

// 2. Extract coupon code from args
let code = extract_coupon_code(&args);

// 3. Validate format
if !code_regex.is_match(&code) { return Cancel("Invalid format"); }

// 4. Check blocked patterns (TEST, FAKE, etc.)
if blocked_regex.is_match(&code) { return Cancel("Blocked"); }

// 5. Check suspicious codes (AAAA, 1234, etc.)
if is_suspicious_code(&code) { return Cancel("Suspicious"); }

// 6. Require validation through sub-agent
if require_validation { 
    // Log requirement, enforce via delegate
}
```

### 3. Config-based Guardrails

```toml
[hooks.coupon_guardrail]
enabled = true
code_pattern = "^[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}$"
require_validation = true
validation_agent = "coupon_validator"
blocked_patterns = ["TEST.*", "FAKE.*", "AAAA.*"]
guarded_tools = ["apply_coupon", "checkout"]
```

## 🚀 การใช้งาน

### 1. Build ZeroClaw

```bash
cd /Users/dome/project/test/zeroclaw
cargo build --release
```

### 2. ตั้งค่า Config

```bash
# Copy config example
mkdir -p ~/.zeroclaw
cp workspace-tokenine/config.example.toml ~/.zeroclaw/config.toml

# Edit config with your API keys
# Set environment variables:
export ANTHROPIC_API_KEY="your-key"
export COUPON_API_ENDPOINT="http://your-api/coupons"
export COUPON_API_TOKEN="your-token"
```

### 3. รัน Bot

```bash
# Set workspace
export ZEROCLAW_WORKSPACE="/Users/dome/project/test/zeroclaw/workspace-tokenine"

# Run
./target/release/zeroclaw agent
```

## 📋 ขั้นตอนการลงทะเบียนคูปอง

1. **ลูกค้าส่ง code** → Main Agent (โทนี่) รับข้อความ
2. **ถามข้อมูลเพิ่ม** → ชื่อ, เบอร์โทร (ถ้ายังไม่มี)
3. **Hook ตรวจสอบ format** → `CouponGuardrailHook` intercept
4. **เรียก Sub-Agent** → `delegate` tool → `coupon_validator` agent
5. **Validator เรียก API** → `http_request` → Coupon API
6. **รับผล** → ส่งผลกลับให้ Main Agent
7. **แจ้งลูกค้า** → Main Agent แจ้งผลตาม status

## ⚠️ ข้อควรระวัง

1. **ห้าม** ให้ Main Agent มี `apply_coupon` ใน `allowed_tools`
2. **ห้าม** ปิด `require_validation` ใน hook config
3. **ห้าม** เพิ่ม `http_request` ใน Main Agent tools (ให้ validator เท่านั้น)
4. **ควร** เก็บ log ทุกการลงทะเบียน (hook จะ log อัตโนมัติ)
5. **ควร** ตั้ง `autonomy.level = "supervised"` เพื่อความปลอดภัย

## 🔧 การปรับแต่ง

### เปลี่ยน Format ของ Code

```toml
[hooks.coupon_guardrail]
code_pattern = "^[A-Z]{3}[0-9]{5}$"  # เปลี่ยน format
```

### เพิ่ม Blocked Patterns

```toml
[hooks.coupon_guardrail]
blocked_patterns = [
    "TEST.*",
    "FAKE.*",
    "YOUR-CUSTOM-PATTERN.*"
]
```

### เพิ่ม Guarded Tools

```toml
[hooks.coupon_guardrail]
guarded_tools = [
    "apply_coupon",
    "checkout",
    "your_custom_tool"
]
```

## 📊 เปรียบเทียบ ZeroClaw vs PicoClaw

| Feature | PicoClaw (Go) | ZeroClaw (Rust) |
|---------|---------------|------------------|
| **RAM** | <10MB | <5MB ✅ |
| **Startup** | <1s | <10ms ✅ |
| **Hook System** | Custom | Trait-based ✅ |
| **before_tool_call** | ✅ | ✅ (Cancel support) |
| **Sub-agent** | Process | Delegate tool ✅ |
| **Type Safety** | Go interfaces | Rust traits ✅ |
| **Async** | Goroutines | Tokio ✅ |

## 📞 ติดต่อ

หากมีปัญหาหรือต้องการปรับแต่งเพิ่มเติม กรุณาติดต่อทีมพัฒนา