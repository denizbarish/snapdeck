# Snapdeck Full Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bir Chrome sekmesindeki sayfanın tamamı, kaydırılarak yakalanır, tek bir PNG'de birleşir, loopback bir köprü üzerinden masaüstüne geçer ve bugünkü yakalamaların girdiği kaydetme ve editör akışına aynen girer. Eklenti yokken masaüstü, kaydırılabilir bir pencereyi kendi kareleriyle birleştirir ve bunu açıkça deneysel diye sunar.

**Architecture:** Köprü mesajları tek bir yerde, `packages/protocol` içinde zod ile tanımlanır; eklenti onu import eder, masaüstü onun aynadaki serde karşılığını taşır ve iki taraf çapraz dil testleriyle birbirine bağlanır. Köprü, `127.0.0.1` üzerinde bir WebSocket sunucusudur ve üç kapısı vardır: el sıkışmada `Origin`, ilk mesajda sürüm ve token, her çerçevede boyut ve şema. Kapılardan biri kapanırsa bağlantı kapanır. Eklenti tarafında ölçüm, kaydırma planı ve birleştirme saf modüllerdir; `chrome` API'sine dokunan kod ince bir kabuktur. `crates/stitch` yalnızca piksel bilir ve elle kurulmuş kare dizileriyle deterministik olarak test edilir.

**Tech Stack:** TypeScript (strict, `noUncheckedIndexedAccess`), zod 4, Vite 6, Manifest V3, Rust 1.85, Tauri 2.11.5, tungstenite 0.30, Vitest 3, Playwright/Chromium.

## Global Constraints

- Hedef platform macOS 14.0+. Kod, yorum, commit mesajı ve arayüz metinleri İngilizce; yalnızca `docs/superpowers/` Türkçe.
- Telemetri yok. Ağa çıkan tek şey güncelleme kontrolüdür ve o Plan 4'ün konusudur. Köprü **ağa çıkmaz**: yalnız `127.0.0.1` dinler.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm lint`, `pnpm test`, `pnpm build` her commit'te temiz.
- `packages/protocol` Tauri, React ve DOM bilmez. Tek bağımlılığı zod'dur. Hem eklenti hem de masaüstünün Rust tarafı ona bakar, biri onu import eder, diğeri onu okuyan testlerle ona bağlanır.
- Gelen PNG **mevcut** akışa girer: `write_capture` ile kaydedilir, `copy_to_clipboard` ile panoya yazılır, `editor::open_editor` ile açılır. İkinci bir kaydetme yolu yazmak yasaktır; kullanıcının `default_format`, `filename_template` ve `open_editor_after_capture` ayarları full-page yakalamaya da aynen uygulanır.
- Yakalama akışı bozulmaz: `capture_region`, `save_edited`, `copy_edited`, izin komutları ve overlay davranışı bu planda değişmez.
- Editörün gizlilik garantisi bozulmaz: `obscure` katmanı hâlâ pikselleri yok eder.

## Bu planın kapsamı dışında

- Firefox, Safari veya başka bir tarayıcı için eklenti (protokol paylaşılabilir, port bu planda değil).
- Chrome Web Store yayını ve eklentinin release hattına eklenmesi.
- Ekran kaydı (kullanıcı kararıyla v2).
- Köprü üzerinden başka bir şey taşımak (bölge yakalama tetikleme, editör senkronu, dosya listesi).
- Eklentinin kendi annotation editörünü açması. `packages/editor` bunun için Tauri'siz tutuldu, ama bu planda eklenti yalnızca PNG üretir.

## Plan formatı hakkında

Bu plan tip tanımlarını, fonksiyon imzalarını ve test durumlarını **birebir** verir: sözleşme budur, uygulayıcı bunları değiştiremez. Gövde kodunu uygulayıcı yazar. Her test için **mutasyon kanıtı** verilir: testi geçiren kodun hangi tek satırı bozulunca testin kırmızıya döneceği. Mutasyon kanıtı olmayan test yazılmaz; testi yazan kişi o satırı gerçekten bozup kırmızıyı görmekle yükümlüdür.

TDD zorunludur: önce test, sonra kod. Her görevin ilk adımı "testleri yaz", ikinci adımı "kırmızıyı gör".

## Dosya sahipliği ve paralellik kuralı

Daha önce iki uygulayıcı aynı paket içinde aynı anda çalıştığında karışık bir ağaç kaldı. Bu planda **aynı sahiplik biriminde aynı anda yalnız bir ajan çalışır**. Birimler:

| Birim | Kapsam |
|---|---|
| U1 | `packages/protocol/**` |
| U2 | `crates/stitch/**` ve kök `Cargo.toml`'un `members` satırı |
| U3 | `apps/desktop/**` (hem `src-tauri` Rust'ı hem `src` TypeScript'i) |
| U4 | `apps/extension/**` |
| U5 | `.github/workflows/**`, `README.md`, `docs/*.md` |

Kök `pnpm-workspace.yaml` hiç değişmez: `apps/*` ve `packages/*` globları yeni paketleri zaten kapsıyor. Kök `Cargo.toml`'a yalnız U2 dokunur (tek satır, `crates/stitch` üyeliği).

---

## File Structure

```
packages/protocol/package.json
packages/protocol/tsconfig.json
packages/protocol/vitest.config.ts
packages/protocol/src/limits.ts            sürüm, port, boyut sınırları
packages/protocol/src/messages.ts          zod şemaları ve ayrıştırıcılar
packages/protocol/src/index.ts             dışa açılan yüzey
packages/protocol/src/*.test.ts

apps/desktop/src-tauri/src/bridge/mod.rs      köprünün dışa açılan yüzeyi
apps/desktop/src-tauri/src/bridge/token.rs    eşleştirme token'ı
apps/desktop/src-tauri/src/bridge/protocol.rs serde aynası, sabitler, hata tipi
apps/desktop/src-tauri/src/bridge/session.rs  oturum durum makinesi, Origin kuralı
apps/desktop/src-tauri/src/bridge/server.rs   loopback dinleyici
apps/desktop/src-tauri/src/bridge/intake.rs   gelen PNG'nin kaydetme yoluna girişi
apps/desktop/src-tauri/src/fullpage.rs        masaüstü fallback sürücüsü
apps/desktop/src/settings/SettingsWindow.tsx  token ve köprü durumu bölümü

apps/extension/package.json
apps/extension/vite.config.ts               service worker + options (ESM)
apps/extension/vite.content.config.ts       content script (IIFE)
apps/extension/public/manifest.json
apps/extension/options.html
apps/extension/scripts/check-manifest.mjs
apps/extension/src/manifest.ts               saf manifest denetimi
apps/extension/src/content/measure.ts        sayfa ölçümü
apps/extension/src/content/plan.ts           kaydırma planı
apps/extension/src/content/sticky.ts         sabit eleman gizleme
apps/extension/src/content/main.ts           content script kabuğu
apps/extension/src/background/throttle.ts    Chrome kotası
apps/extension/src/background/composite.ts   katman birleştirme
apps/extension/src/background/capture.ts     yakalama döngüsü
apps/extension/src/background/bridge.ts      köprü istemcisi
apps/extension/src/background/fallback.ts    indirme fallback'i
apps/extension/src/background/main.ts        service worker kabuğu
apps/extension/src/options/main.ts           seçenekler sayfası
apps/extension/src/**/*.test.ts

crates/stitch/Cargo.toml
crates/stitch/src/lib.rs                     stitch(), Image, ScrollHint
crates/stitch/src/error.rs                   StitchError
crates/stitch/src/align.rs                   satır imzaları ve korelasyon
crates/stitch/src/compose.rs                 karelerin yapıştırılması
```

---

## Yeni bağımlılıklar ve neden

| Bağımlılık | Nerede | Neden bu |
|---|---|---|
| `zod` ^4.6.1 | `packages/protocol` | Brief'in şartı. Şema hem tip hem çalışma zamanı doğrulaması üretir, yani sözleşme tek yerde durur ve güven sınırında gerçekten kontrol edilir. |
| `tungstenite` 0.30 | `apps/desktop/src-tauri` | Tauri ekosisteminde sunucu tarafı bir WebSocket yok: `tauri-plugin-websocket` istemcidir, `tauri-plugin-localhost` uygulamanın kendi sayfasını sunar. `accept_hdr_with_config` el sıkışma başlıklarını (yani `Origin`'i) verir ve `WebSocketConfig::max_message_size` boyut sınırını verir; ihtiyaç duyulan iki güvenlik kancası tam olarak bunlar. Varsayılan özellikleri TLS taşımaz (handshake, http, httparse, sha1, data-encoding), MSRV 1.85 çalışma alanının `rust-version` değeriyle aynı. Eşzamanlı (sync) sürüm seçildi: köprü en fazla birkaç bağlantı görür, oturum başına bir iş parçacığı bir async görev kurgusundan basittir ve açık bir `tokio` bağımlılığı gerektirmez. |
| `getrandom` 0.4 | `apps/desktop/src-tauri` | 32 baytlık eşleştirme token'ı doğrudan sistem CSPRNG'sinden gelir. Tek fonksiyon (`fill`), tohumlanan bir kullanıcı alanı üreteci bir kerelik sırra hiçbir şey katmaz, ve 0.4.3 zaten `Cargo.lock` içinde. |
| `base64` 0.22 | `apps/desktop/src-tauri` | Dev-dependency'den gerçek bağımlılığa yükseltilir. PNG, JSON çerçevesinde base64 taşınır. 0.22.1 zaten `Cargo.lock` içinde (`tauri-plugin-updater` üzerinden). |
| `thiserror` 2 | `apps/desktop/src-tauri` | `crates/capture/src/error.rs` deseninin `BridgeError` karşılığı için. Zaten çalışma alanı bağımlılığı, `thiserror.workspace = true` ile alınır. |
| `@types/chrome` ^0.2.9 | `apps/extension` (dev) | MV3 API tipleri. |
| `snapdeck-capture` (path) | `crates/stitch` | `Frame` ve `PixelFormat` tipleri. Yakalama crate'i Tauri bilmez ve `Frame` saf veridir, yani stitch'in "yalnız görüntü verisi bilir" kuralı bozulmaz; kendi paralel kare tipini tanımlamak iki tarafta dönüşüm kodu doğururdu. |
| `snapdeck-stitch` (path), `core-graphics` (workspace) | `apps/desktop/src-tauri`, Görev 12 | Fallback sürücüsü. `core-graphics` 0.25 zaten çalışma alanı bağımlılığı. |

**Eklenmeyenler ve neden:**

- `@crxjs/vite-plugin`: `apps/desktop/vite.config.ts` zaten çok girişli düz Vite kullanıyor. Aynı deseni iki config ile sürdürmek (ESM service worker + IIFE content script) yeni bir build aracı taşımaktan ucuz, ve manifest elde yazıldığı için üretilen bir manifest ile gerçek dosya adları arasında sessiz bir kayma olamaz. Denetimi `check-manifest.mjs` yapar.
- `rustfft`: aranan yer değiştirme tek boyutlu ve kare yüksekliğiyle sınırlı. Faz korelasyonunun asıl kazandırdığı şey, genlikle normalizasyonun aydınlanma farkını yok saymasıdır; bunun uzamsal alandaki karşılığı sıfır ortalamalı normalize edilmiş çapraz korelasyondur (ZNCC) ve `align`'ın kullandığı budur. `align`'ın A3 testi bu iddianın kanıtıdır. Gerçek yakalamalarda yetmezse FFT'ye geçmek `align`'ın imzasını değiştirmez.
- `tokio-tungstenite`: yukarıda.
- `subtle`: sabit zamanlı karşılaştırma altı satır ve testi var.
- React (`apps/extension`): seçenekler sayfası bir metin alanı, iki düğme ve bir durum satırı. React eklentinin tek çalışma zamanı bağımlılığı olurdu.

---

### Task 1: `packages/protocol`, köprü mesaj sözleşmesi

**Birim:** U1. **Bağımlılık:** yok.

**Files:** `packages/protocol/{package.json,tsconfig.json,vitest.config.ts}`, `src/{limits.ts,messages.ts,index.ts}`, `src/{limits,messages}.test.ts`

`package.json`, `@snapdeck/editor` ile aynı biçimdedir: `private`, `"main": "./src/index.ts"`, build adımı yok, `"test": "vitest run"`, `"lint": "tsc --noEmit"`. `tsconfig.json`, editörünkinin kopyasıdır (`strict`, `noUncheckedIndexedAccess`, `types: []`). `vitest.config.ts` tek proje, node ortamı.

**Interfaces (birebir):**

```ts
// limits.ts
export const PROTOCOL_VERSION = 1
export const BRIDGE_PORT = 51837
export const MAX_PNG_BYTES = 41_943_040
export const MAX_MESSAGE_BYTES = 67_108_864
export const MAX_IMAGE_PIXELS = 64_000_000
export const EXTENSION_ORIGIN_PREFIX = 'chrome-extension://'
export const CLOSE_POLICY_VIOLATION = 1008
```

```ts
// messages.ts
import { z } from 'zod'

export const errorCodeSchema = z.enum([
  'unsupportedProtocolVersion',
  'unauthorized',
  'malformedMessage',
  'saveFailed',
])
export type ErrorCode = z.infer<typeof errorCodeSchema>

export const helloSchema = z.strictObject({
  type: z.literal('hello'),
  protocolVersion: z.number().int(),
  token: z.string().min(1).max(256),
  client: z.strictObject({ name: z.string().min(1).max(64), version: z.string().min(1).max(32) }),
})

export const fullPageSchema = z.strictObject({
  type: z.literal('fullPage'),
  requestId: z.string().min(1).max(64),
  page: z.strictObject({ url: z.string().min(1).max(2048), title: z.string().max(1024) }),
  image: z.strictObject({
    pngBase64: z.string().min(1),
    width: z.number().int().positive(),
    height: z.number().int().positive(),
    devicePixelRatio: z.number().positive(),
  }),
  truncated: z.boolean(),
})

export const clientMessageSchema = z.discriminatedUnion('type', [helloSchema, fullPageSchema])
export type ClientMessage = z.infer<typeof clientMessageSchema>
export type Hello = z.infer<typeof helloSchema>
export type FullPage = z.infer<typeof fullPageSchema>

export const readySchema = z.strictObject({
  type: z.literal('ready'),
  protocolVersion: z.number().int(),
  app: z.strictObject({ name: z.string(), version: z.string() }),
})
export const acceptedSchema = z.strictObject({
  type: z.literal('accepted'),
  requestId: z.string(),
  savedPath: z.string().nullable(),
})
export const errorMessageSchema = z.strictObject({
  type: z.literal('error'),
  requestId: z.string().nullable(),
  code: errorCodeSchema,
  message: z.string(),
})
export const serverMessageSchema = z.discriminatedUnion('type', [
  readySchema,
  acceptedSchema,
  errorMessageSchema,
])
export type ServerMessage = z.infer<typeof serverMessageSchema>

export class ProtocolError extends Error {
  readonly code: ErrorCode
  constructor(code: ErrorCode, message: string)
}

export function parseClientMessage(raw: string): ClientMessage
export function parseServerMessage(raw: string): ServerMessage
export function encode(message: ClientMessage | ServerMessage): string
```

**Kurallar:**

- `z.strictObject`, `z.object` değil. Bilinmeyen bir alan şema hatasıdır. Sebep: bu bir güven sınırıdır ve "fazladan alanı sessizce at" davranışı, iki tarafın farklı şeyler konuştuğunu gizler. Alan eklemenin yolu `PROTOCOL_VERSION`'ı artırmaktır, sessizce yeni bir alan sızdırmak değil.
- `parse*` fonksiyonları asla `SyntaxError` veya `ZodError` sızdırmaz: her ikisi de `ProtocolError('malformedMessage', …)` olur. Çağıran tek bir tip yakalar.
- `encode`, sonucun UTF-8 bayt uzunluğunu `TextEncoder` ile ölçer ve `MAX_MESSAGE_BYTES`'ı aşarsa fırlatır, kaç bayt olduğunu ve sınırı söyler. `string.length` ile ölçmek yanlıştır: çok baytlı bir sayfa başlığı sınırın altında görünür ve mesaj karşı tarafta 1009 ile kapanır.
- `MAX_MESSAGE_BYTES`, `MAX_PNG_BYTES` boyutunda bir PNG'nin base64'ünü artı JSON zarfını taşımak zorundadır. İkisi bağımsız sabitler değil, biri diğerinin sonucudur; bunu bir test tutar.

- [ ] **Step 1: Testleri yaz (bunlar sözleşmedir, birebir)**

`messages.test.ts`:

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| M1 `parseClientMessage` geçerli bir `hello`'yu ayrıştırır ve `token` alanını taşır | mutlu yol | `helloSchema`'dan `token` satırını sil: ayrıştırılan nesnede alan kalmaz, iddia kırmızı |
| M2 Fazladan `extra` alanı taşıyan `hello` `ProtocolError('malformedMessage')` ile reddedilir | bilinmeyen alan bir şema hatasıdır | `z.strictObject` → `z.object`: ayrıştırma başarılı olur, `toThrow` kırmızı |
| M3 `type` alanı olmayan JSON reddedilir | ayrık birleşimin ayırt edici alanı zorunlu | `parseClientMessage` içindeki `safeParse` sonuç kontrolünü kaldırıp `data`'yı doğrudan döndür: kırmızı |
| M4 JSON olmayan metin `ProtocolError('malformedMessage')` üretir, `SyntaxError` sızdırmaz | tek hata tipi | `JSON.parse`'ı try/catch dışına al: `SyntaxError` fırlar, tip iddiası kırmızı |
| M5 `encode`, çok baytlı Türkçe karakterlerle doldurulmuş `page.title` yüzünden sınırı aşan bir mesajı reddeder ve bayt sayısını söyler | boyut baytla ölçülür, karakterle değil | `TextEncoder().encode(json).length` → `json.length`: mesaj sınırın altında görünür, `toThrow` kırmızı |
| M6 `parseServerMessage`, bilinmeyen bir `code` taşıyan `error` mesajını reddeder | hata kodları kapalı bir küme | `errorCodeSchema` → `z.string()`: kırmızı |
| M7 `fullPageSchema` `width: 0` ve `width: -1` değerlerini reddeder | sıfır boyutlu tuval kaydetme yolunu bozar | `.positive()` sil: kırmızı |
| M8 `parseServerMessage` `accepted` içinde `savedPath: null` kabul eder | uygulama kaydedemeyip panoya yazdığında verdiği cevap | `.nullable()` sil: kırmızı |

`limits.test.ts`:

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| L1 `Math.ceil(MAX_PNG_BYTES / 3) * 4 < MAX_MESSAGE_BYTES` | tel sınırı, taşınacak en büyük PNG'nin base64'ünü artı zarfı alır | `MAX_PNG_BYTES`'ı iki katına çıkar: kırmızı |
| L2 `MAX_IMAGE_PIXELS < 268_435_456` (Chrome'un masaüstündeki azami tuval alanı) | eklentinin üreteceği tuval tarayıcının kabul edebileceği boyutta | `MAX_IMAGE_PIXELS`'i 300 milyona çıkar: kırmızı |
| L3 `BRIDGE_PORT` IANA dinamik aralığındadır (49152..65535) | sabit port kayıtlı bir servisi çalmaz | portu 8080 yap: kırmızı |

- [ ] **Step 2: Kırmızıyı gör, sonra uygula.**
- [ ] **Step 3:** `pnpm --filter @snapdeck/protocol test` ve `lint` temiz.
- [ ] **Step 4: Commit** `feat(protocol): add the bridge message contract`

---

### Task 2: Köprü çekirdeği, token ve mesaj aynası

**Birim:** U3. **Bağımlılık:** Görev 1.

**Files:** `apps/desktop/src-tauri/src/bridge/{mod.rs,token.rs,protocol.rs}`, `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/src/lib.rs` (yalnız `mod bridge;`)

**Interfaces (birebir):**

```rust
// bridge/token.rs
/// Bytes of system randomness behind a pairing token.
pub const TOKEN_BYTES: usize = 32;

/// A fresh pairing token: `TOKEN_BYTES` bytes from the system CSPRNG, lowercase hex.
pub fn generate_token() -> Result<String, String>;

/// Whether `presented` is `expected`, in time that does not depend on how much
/// of it matched.
pub fn tokens_match(expected: &str, presented: &str) -> bool;
```

```rust
// bridge/protocol.rs
pub const PROTOCOL_VERSION: u32 = 1;
pub const BRIDGE_PORT: u16 = 51837;
pub const MAX_PNG_BYTES: usize = 41_943_040;
pub const MAX_MESSAGE_BYTES: usize = 67_108_864;
pub const MAX_IMAGE_PIXELS: u64 = 64_000_000;
pub const EXTENSION_ORIGIN_PREFIX: &str = "chrome-extension://";
pub const CLOSE_POLICY_VIOLATION: u16 = 1008;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelloTag { Hello }

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FullPageTag { FullPage }

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientInfo { pub name: String, pub version: String }

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageInfo { pub url: String, pub title: String }

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImagePayload {
    pub png_base64: String,
    pub width: u32,
    pub height: u32,
    pub device_pixel_ratio: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Hello {
    pub r#type: HelloTag,
    pub protocol_version: u32,
    pub token: String,
    pub client: ClientInfo,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FullPage {
    pub r#type: FullPageTag,
    pub request_id: String,
    pub page: PageInfo,
    pub image: ImagePayload,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage { Hello(Hello), FullPage(FullPage) }

impl ClientMessage {
    /// Parses one text frame. The only place a bridge frame is deserialised.
    pub fn parse(raw: &str) -> Result<Self, BridgeError>;
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo { pub name: String, pub version: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    UnsupportedProtocolVersion,
    Unauthorized,
    MalformedMessage,
    SaveFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ServerMessage {
    Ready { protocol_version: u32, app: AppInfo },
    Accepted { request_id: String, saved_path: Option<String> },
    Error { request_id: Option<String>, code: ErrorCode, message: String },
}

impl ServerMessage {
    pub fn encode(&self) -> String;
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum BridgeError {
    #[error("the frame is not valid JSON: {0}")]
    NotJson(String),
    #[error("the frame does not match the protocol schema: {0}")]
    Schema(String),
    #[error("unknown message type: {0}")]
    UnknownType(String),
    #[error("the extension speaks protocol version {presented}, which this build does not")]
    VersionMismatch { presented: u32 },
    #[error("the pairing token does not match")]
    Unauthorized,
}

impl BridgeError {
    /// The code this failure is reported to the extension under.
    pub fn code(&self) -> ErrorCode;
}
```

**Kurallar:**

- `#[serde(tag = "type")]` ile bir enum **kullanılmaz**: serde'de dahili etiketleme ile `deny_unknown_fields` birleşmez, ve bilinmeyen alan katılığı bu şemayı bir şekil kontrolünden ayıran şeyin yarısıdır. Her mesaj kendi struct'ıdır ve `type` alanını tek varyantlı bir etiket tipiyle taşır, böylece `deny_unknown_fields`'a artan bir alan kalmaz. `packages/protocol` aynı sebeple `z.strictObject` kullanıyor.
- `ClientMessage::parse` önce `type` alanını okur (`serde_json::Value`'dan bir kez), sonra tam struct'a ayrıştırır. `type` tanınmıyorsa `UnknownType`, JSON değilse `NotJson`, geri kalan her şey `Schema`.
- `tokens_match` erken dönmez: uzunluklar farklıysa yine de her iki dizenin baytları üzerinden yürür ve sonucu bir XOR toplayıcısıyla verir. Basit bir `==` sızdırmaz ama `zip` ile yazılmış bir döngü uzunluk farkını sessizce keser, ki bu bir önekin tam token sayılması demektir.
- `BridgeError`, `crates/capture/src/error.rs` desenini izler: `thiserror`, kullanıcıya okunabilir `#[error]` metinleri.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| B1 `generate_token` 64 küçük harf hex karakter döner ve iki çağrı aynı değeri vermez | token gerçekten rastgele ve beklenen biçimde | `TOKEN_BYTES`'ı 4 yap: uzunluk iddiası kırmızı |
| B2 `tokens_match` aynı token için `true`, tek karakteri değişmiş için `false` | temel doğruluk | karşılaştırmayı `true` sabitine çevir: kırmızı |
| B3 `tokens_match("ab", "abcd")` ve `tokens_match("abcd", "ab")` `false` döner ve panik atmaz | önek tam token sayılmaz | uzunluk kontrolünü kaldırıp yalnız `zip` ile yürü: `("ab","abcd")` `true` döner, kırmızı |
| B4 `ClientMessage::parse` geçerli bir `hello`'yu `Hello` varyantı olarak döner | mutlu yol | `HelloTag` yerine `String` koy ve etiketi doğrulamayı bırak: B5 kırmızı |
| B5 `type: "fullPage"` gövdesi `Hello` alanlarıyla gelirse `Schema` hatası olur | etiket ile gövde birbirini tutmak zorunda | yukarıdaki |
| B6 Fazladan alan taşıyan `hello` `Schema` ile reddedilir | bilinmeyen alan katılığı | `deny_unknown_fields` sil: kırmızı |
| B7 JSON olmayan metin `NotJson`, geçerli JSON ama eksik alan `Schema` üretir; ikisi ayrı | kullanıcıya iki farklı şey söylenir | iki kolu tek `Schema`'ya indir: kırmızı |
| B8 `type` tanınmıyorsa `UnknownType` ve mesaj tipi adını taşır | bilinmeyen mesaj sessizce yutulmaz | `UnknownType` kolunu `Schema`'ya çevir: kırmızı |
| B9 `ServerMessage::Error { request_id: None, .. }.encode()` tam olarak `{"type":"error","requestId":null,"code":"saveFailed","message":"..."}` üretir | tel biçimi camelCase | `rename_all_fields = "camelCase"` sil: `request_id` yazılır, kırmızı |
| B10 `BridgeError::code()` her varyantı doğru `ErrorCode`'a eşler; `NotJson`, `Schema` ve `UnknownType` üçü de `MalformedMessage` olur | eklentiye giden kod kümesi kapalı | `VersionMismatch` kolunu `Unauthorized`'a çevir: kırmızı |
| B11 **Çapraz dil:** `include_str!("../../../../../packages/protocol/src/limits.ts")` okunur; `PROTOCOL_VERSION`, `BRIDGE_PORT`, `MAX_PNG_BYTES`, `MAX_MESSAGE_BYTES`, `MAX_IMAGE_PIXELS` beşi de burada yazılan değerlerle o dosyada geçer | iki dilin sabitleri ayrışamaz | Rust'taki `BRIDGE_PORT`'u değiştir: kırmızı. TS'teki değiştir: yine kırmızı |
| B12 **Çapraz dil:** `include_str!(".../messages.ts")` okunur; her `ErrorCode` varyantının camelCase adı `errorCodeSchema` listesinde geçer | hata kodu kümesi iki tarafta aynı | Rust'a beşinci bir varyant ekle, TS'e ekleme: kırmızı |

B11 ve B12, `output::the_jpeg_quality_matches_the_one_the_editor_encodes_at` ile aynı desendir: iddia iki dil arasındadır, iki Rust sabiti arasında değil.

- [ ] **Step 2:** Kırmızıyı gör, uygula.
- [ ] **Step 3:** `cargo fmt`, `cargo clippy -D warnings`, `cargo test -p snapdeck` temiz.
- [ ] **Step 4: Commit** `feat(app): add the bridge protocol and pairing token`

---

### Task 3: Köprü sunucusu, loopback dinleyici ve güvenlik kapısı

**Birim:** U3. **Bağımlılık:** Görev 2.

**Files:** `apps/desktop/src-tauri/src/bridge/{server.rs,session.rs,mod.rs}`, `Cargo.toml` (tungstenite), `src/lib.rs`, `src/state.rs`

**Interfaces (birebir):**

```rust
// bridge/session.rs

/// Whether a handshake's `Origin` may open a bridge session.
///
/// Pure, and separated from the socket for the reason `lib::adopt_shortcuts`
/// is: this is the whole of the rule, and a live handshake is not something a
/// unit test can arrange.
pub fn origin_is_allowed(origin: Option<&str>) -> bool;

/// One session's transport, so the state machine can be driven without a socket.
pub trait Frames {
    fn recv_text(&mut self) -> Result<String, String>;
    fn send_text(&mut self, text: &str) -> Result<(), String>;
    fn close(&mut self, code: u16, reason: &str);
}

/// What a connection is allowed to do, and who decides.
///
/// Injected rather than read from an `AppHandle`, so the gate can be tested
/// against a real socket without a running Tauri application.
pub trait BridgePolicy: Send + Sync + 'static {
    fn token(&self) -> String;
    fn app_info(&self) -> AppInfo;
    /// Handles one accepted page. `Ok(None)` means the picture reached the
    /// clipboard but not the disk, exactly as a region capture can.
    fn deliver(&self, message: &FullPage) -> Result<Option<String>, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEnd {
    /// The peer closed, or the transport ended.
    Closed,
    /// Refused, with the code the extension was told.
    Refused(ErrorCode),
}

/// Runs one session to its end.
pub fn run_session<F: Frames>(frames: &mut F, policy: &dyn BridgePolicy) -> SessionEnd;
```

```rust
// bridge/server.rs

#[derive(Debug, Clone, Copy)]
pub struct BridgeLimits {
    pub max_message_bytes: usize,
    pub max_sessions: usize,
    pub handshake_timeout: Duration,
}

impl Default for BridgeLimits {
    /// The protocol's own numbers. The application never builds this by hand.
    fn default() -> Self;
}

/// A running listener. Dropping it stops accepting and ends open sessions.
pub struct BridgeServer { /* private */ }

impl BridgeServer {
    /// Binds `127.0.0.1:port` and serves until dropped.
    ///
    /// `port` is `protocol::BRIDGE_PORT` in the application and 0 in tests, so
    /// the operating system picks a free one and `local_addr` reports it.
    pub fn start(
        port: u16,
        policy: Arc<dyn BridgePolicy>,
        limits: BridgeLimits,
    ) -> Result<Self, String>;

    pub fn local_addr(&self) -> SocketAddr;
}
```

**Kurallar:**

- Bağlanma adresi `Ipv4Addr::LOCALHOST`'tur, hiçbir yerde `0.0.0.0` veya `UNSPECIFIED` geçmez.
- El sıkışma `accept_hdr_with_config` ile karşılanır. Geri çağrı `Origin`'i okur; `origin_is_allowed` `false` derse HTTP 403 döndürülür ve WebSocket hiç kurulmaz.
- `origin_is_allowed` kuralı: değer var olmalı, `chrome-extension://` ile başlamalı ve geri kalanı 32 karakterlik bir eklenti kimliği olmalı (`a`..`p`). Eksik `Origin`, `null`, `file://`, `http://`, `https://` reddedilir. Kimliğin kendisi **sabitlenmez**: paketlenmemiş bir geliştirme yüklemesi ile mağaza yayınının kimlikleri farklıdır ve ikisini de kabul etmenin tek yolu şema ve biçim kontrolüdür. Asıl tehdit, sıradan bir web sayfasının JavaScript'inin loopback sokete bağlanmasıdır; tarayıcı bu sayfaya `Origin` uydurtmaz, dolayısıyla bu kural onu kesin olarak dışarıda tutar. Token, kuralın ikinci yarısıdır.
- `WebSocketConfig` `max_message_size` ve `max_frame_size` için `limits.max_message_bytes` alır. Sınırı aşan çerçeveyi tungstenite kendi kapatır (1009); uygulama onu yalnız raporlar.
- `run_session` sırası, birebir:
  1. İlk çerçeve `hello` olmalı. Değilse `error{malformedMessage}` gönderilir ve 1008 ile kapatılır.
  2. `protocol_version != PROTOCOL_VERSION` ise `error{unsupportedProtocolVersion}` gönderilir ve kapatılır. **Token'dan önce**, çünkü sürüm uyuşmazlığı kullanıcıya sürüm uyuşmazlığı olarak söylenmeli; yetki hatası diye söylenirse kullanıcı token'ı yeniden yapıştırmakla vakit kaybeder. Köprünün varlığı zaten el sıkışmanın başarısından belli, yani bu sırada saklanan bir şey yok.
  3. `tokens_match` başarısızsa `error{unauthorized}` ve kapatma.
  4. Başarıda `ready` gönderilir.
  5. Sonraki her `fullPage` `policy.deliver`'a gider. Başarıda `accepted{request_id, saved_path}`, hatada `error{saveFailed, request_id}` gönderilir ve **oturum açık kalır**: başarısız bir kaydetme protokol ihlali değildir, kullanıcı yeniden deneyebilmelidir.
  6. `ready` sonrası ikinci bir `hello`, ya da herhangi bir ayrıştırma hatası, `error` + kapatma.
- El sıkışmadan sonra ilk `hello`'ya kadar sokete `limits.handshake_timeout` okuma zaman aşımı konur, sonra kaldırılır. Açılıp susan bir bağlantı bir iş parçacığını süresiz tutmaz.
- Eşzamanlı oturum sayısı `limits.max_sessions` ile sınırlıdır. Aşıldığında bağlantı kabul edilip hemen kapatılır; mevcut oturumlar etkilenmez.
- `lib::run`'un `setup`'ı sunucuyu `BridgeLimits::default()` ile başlatır ve `BridgeServer`'ı `AppState`'te tutar. Bağlanma başarısızsa (port dolu) `report::report_failure` ile kullanıcıya söylenir ve uygulama normal çalışmaya devam eder: köprü yoksa da yakalama çalışır.

- [ ] **Step 1: Saf testleri yaz (`session.rs`, sahte `Frames` ve sahte `BridgePolicy`)**

Sahte politika `deliver` çağrılarını kaydeder; sahte `Frames` gönderilen metinleri ve `close` çağrısını kaydeder.

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| S1 `origin_is_allowed(Some("chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))` `true` | gerçek eklenti kaynağı kabul edilir | kimlik alfabesi kontrolünü aşırı daraltırsan kırmızı |
| S2 `None`, `""`, `"null"`, `"http://localhost:1420"`, `"https://evil.example"`, `"file://"` altısı da `false` | **güvenlik:** hiçbir web sayfası köprüye bağlanamaz | `origin.is_none()` dalını `true` yap: kırmızı. `starts_with` kontrolünü sil: `http://localhost` geçer, kırmızı |
| S3 `"chrome-extension://short"` ve `"chrome-extension://"` `false` | kimlik biçimi de kontrol edilir | uzunluk/alfabe kontrolünü sil: kırmızı |
| S4 **Güvenlik:** ilk çerçevesi `fullPage` olan oturum `Refused(MalformedMessage)` ile biter, `deliver` **hiç** çağrılmaz, `close` çağrılır | token'sız bağlantı hiçbir şey teslim edemez | "ilk çerçeve hello olmalı" kontrolünü sil: `deliver` çağrılır, kırmızı |
| S5 **Güvenlik:** yanlış token `Refused(Unauthorized)` üretir, `ready` **hiç** gönderilmez, `deliver` çağrılmaz | yanlış token reddi | `tokens_match` çağrısını `true` ile değiştir: kırmızı |
| S6 **Güvenlik:** hem yanlış sürüm hem yanlış token veren `hello`, `Refused(UnsupportedProtocolVersion)` üretir | sürüm kontrolü token'dan önce | iki kontrolün sırasını değiştir: kırmızı |
| S7 **Güvenlik:** `{"type":"hello"}` gibi eksik alanlı bir çerçeve `Refused(MalformedMessage)` üretir ve bağlantı kapanır | şema dışı mesaj bağlantıyı kapatır | ayrıştırma hatasında `continue` et (kapatma): kırmızı |
| S8 Doğru `hello` sonrası `ready` gönderilir; ardından gelen iki `fullPage`'in **ikisi de** `deliver`'a ulaşır ve ikisine de `accepted` döner | başarılı oturum açık kalır | `accepted` sonrası `close` çağır: ikinci teslim iddiası kırmızı |
| S9 `deliver` `Err` döndüğünde `error{saveFailed}` gönderilir, `request_id` doğru taşınır ve oturum **kapanmaz**: sonraki `fullPage` yine teslim edilir | başarısız kaydetme protokol ihlali değil | `saveFailed` kolunda `close` çağır: kırmızı |
| S10 `deliver` `Ok(None)` döndüğünde `accepted{saved_path: null}` gönderilir | yalnız panoya yazılan bir yakalama da bir cevaptır | `Ok(None)`'ı hata gibi ele al: kırmızı |
| S11 `ready` sonrası ikinci bir `hello` `Refused(MalformedMessage)` üretir | tek el sıkışma | ikinci `hello`'yu yok say: kırmızı |

- [ ] **Step 2: Soket testlerini yaz (`server.rs`, gerçek tungstenite istemcisi, `start(0, …)`)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| S12 `local_addr().ip() == Ipv4Addr::LOCALHOST`, **ve** `include_str!("server.rs")` `"0.0.0.0"` ile `UNSPECIFIED` içermez | **güvenlik:** köprü dış ağdan erişilemez | `Ipv4Addr::LOCALHOST` → `Ipv4Addr::UNSPECIFIED`: iki iddia da kırmızı |
| S13 **Güvenlik:** `Origin` başlığı olmayan el sıkışma HTTP 403 alır, WebSocket kurulmaz | eksik `Origin` reddi | `accept_hdr_with_config` geri çağrısında koşulsuz `Ok(response)` dön: kırmızı |
| S14 **Güvenlik:** `Origin: https://evil.example` taşıyan el sıkışma 403 alır | yanlış `Origin` reddi | yukarıdaki |
| S15 **Güvenlik:** `limits.max_message_bytes` 1024 verilen bir sunucuya 2048 baytlık çerçeve gönderen istemci bağlantının kapandığını görür ve `deliver` çağrılmaz | boyut sınırı aşımı | `max_message_size(Some(..))` → `None`: mesaj kabul edilir, `deliver` çağrılır, kırmızı |
| S16 `BridgeLimits::default().max_message_bytes == protocol::MAX_MESSAGE_BYTES` **ve** `include_str!("../lib.rs")` sunucuyu `BridgeLimits::default()` ile başlatır | uygulama gerçekten protokolün sınırıyla dinler | `default()` içindeki sayıyı elle yaz: kırmızı |
| S17 `max_sessions` 1 verilen sunucuda ikinci bağlantı hemen kapatılır, ilki çalışmaya devam eder; ilki bitince üçüncü bağlantı kabul edilir | eşzamanlılık sınırı ve sayaç geri verme | oturum bitiminde sayaç azaltmasını sil: üçüncü bağlantı iddiası kırmızı |
| S18 Mutlu yol: doğru `Origin` + doğru token ile bir `fullPage` `deliver`'a ulaşır, istemci `accepted` alır | uçtan uca soket yolu | `ready` göndermeyi kaldır: istemci beklerken zaman aşımı, kırmızı |
| S19 Zaten bağlı bir port verilen `start`, port numarasını içeren okunabilir bir `Err` döner | port çakışması kullanıcıya söylenir | `map_err`'i `expect`'e çevir: test panikle kırmızı |
| S20 `handshake_timeout` 100 ms verilen sunucuda, bağlanıp hiçbir şey göndermeyen istemcinin oturumu kendiliğinden biter | susan bağlantı iş parçacığı tutmaz | zaman aşımı ayarını sil: test zaman aşımına uğrar, kırmızı |

- [ ] **Step 3:** Uygula, `lib.rs`'e bağla, `AppState`'e `BridgeServer` alanını ekle.
- [ ] **Step 4:** Tüm kapılar temiz.
- [ ] **Step 5: Commit** `feat(app): listen for the browser extension on loopback`

---

### Task 4: Köprü teslimi, gelen PNG'nin kaydetme ve editör akışına girmesi

**Birim:** U3. **Bağımlılık:** Görev 3.

**Files:** `apps/desktop/src-tauri/src/bridge/intake.rs`, `bridge/mod.rs`, `src/lib.rs`

**Interfaces (birebir):**

```rust
// bridge/intake.rs

#[derive(Debug, Clone, Copy)]
pub struct IntakeLimits { pub max_png_bytes: usize, pub max_pixels: u64 }

impl Default for IntakeLimits {
    /// The protocol's own numbers.
    fn default() -> Self;
}

/// Turns a bridge payload into the frame the capture path already knows.
///
/// The PNG is decoded rather than written straight through, for three reasons
/// stated once here: the user's format setting has to apply to a full page
/// exactly as it applies to a region; the clipboard needs pixels; and a decode
/// is the only thing that checks that the picture a message declares is the
/// picture it carries.
pub fn frame_from_png(
    png_base64: &str,
    declared: &ImagePayload,
    limits: IntakeLimits,
) -> Result<Frame, String>;

/// What a delivered page did to the disk and the clipboard.
#[derive(Debug, PartialEq)]
pub struct Delivered {
    pub path: Option<PathBuf>,
    /// What the user has to be told, if anything.
    pub complaint: Option<String>,
}

/// Saves and copies one delivered page, with the clipboard injected.
///
/// Split out for the reason `write_edited` is: the claim worth testing is that
/// a full page follows exactly the rules a region capture follows, and
/// everything around it needs a live application.
pub fn deliver(
    frame: &Frame,
    directory: &Path,
    settings: &Settings,
    clipboard: impl FnOnce(&Frame) -> Result<(), String>,
) -> Delivered;

/// The `BridgePolicy` the application runs with.
pub struct AppPolicy { /* holds an AppHandle */ }

impl BridgePolicy for AppPolicy { /* token, app_info, deliver */ }
```

**Kurallar:**

- `frame_from_png` sırası: base64 uzunluğundan çözülmüş boyutu tahmin et ve `max_png_bytes`'ı aşıyorsa **çözmeden** reddet; base64 çöz; `image::ImageReader` ile `Limits`'i `max_pixels`'e ayarlayarak çöz; çözülen boyut `declared.width`/`declared.height` ile birebir aynı değilse reddet.
- Üretilen `Frame`: `pixel_format: PixelFormat::Rgba8`, `stride: width as usize * 4`, `scale_factor: declared.device_pixel_ratio`, `captured_at: SystemTime::now()`.
- `AppPolicy::deliver`, `capture_and_write`'ın yaptığını yapar ve aynı sırayla: ayarları bir kez oku, `settings::resolve_save_directory`, `create_dir_all` (best effort), **önce pano sonra dosya**, iki sonucu birlikte tart. Dosya yazılamadıysa pano yine de kalır ve bu bildirilir. `?` ile erken dönüş yoktur.
- `open_editor_after_capture` açıksa ve bir dosya yazıldıysa `editor::open_editor(app, path, width, height, scale)` çağrılır. Ölçek, kareyle gelen `device_pixel_ratio`'dur, birincil monitörünki değil.
- `truncated: true` geldiğinde, kaydetme başarılı olsa bile `report::report_failure` ile kullanıcıya "sayfa çok uzundu, çıktı kesildi" denir. Sessizce eksik bir görüntü verilmez (spec 9).
- `commands.rs` **değişmez**. Köprü kendi modülünden çalışır; mevcut komutlara dokunulmaz.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| I1 Elle kurulmuş 4x3 bir PNG'nin base64'ü doğru genişlik, yükseklik, `stride` ve `scale_factor` ile çözülür ve `to_rgba8()` başarılı olur | mutlu yol ve `stride` doğruluğu | `stride`'ı `width * 3` yaz: `to_rgba8` reddeder, kırmızı |
| I2 Bildirilen boyut gerçek boyuttan farklıysa reddedilir ve mesaj iki boyutu da söyler | **güvenlik:** mesaj kendi yüküyle tutarlı olmalı | karşılaştırmayı sil: kırmızı |
| I3 `max_png_bytes: 16` ile 1 KB'lik bir base64 **çözülmeden** reddedilir, hata sınırı anar | boyut sınırı çözmeden önce | kontrolü tamamen sil: hata mesajı değişir, kırmızı |
| I4 Bozuk base64 reddedilir ve mesaj bunu söyler | hatalı kodlama yutulmaz | `decode` hatasını yutup boş `Vec` dön: kırmızı |
| I5 `max_pixels: 4` ile geçerli bir 4x3 PNG reddedilir ve mesaj piksel sınırını anar | **güvenlik:** decompression bomb kapısı | `Limits`'i decoder'a vermeyi bırak: kırmızı |
| I6 `deliver`, `default_format: Png` ile `.png`, `default_format: Jpeg` ile `.jpg` yazar | kullanıcının format ayarı full-page'e de uygulanır | formatı `SaveFormat::Png` sabitine çevir: JPEG iddiası kırmızı |
| I7 `deliver`, `filename_template`'i kullanır ve aynı ada ikinci kez yazıldığında ` 2` ekler, ilk dosyayı bozmaz | mevcut kaydetme yolunun yeniden kullanıldığının kanıtı | `save_capture_without_overwriting` yerine `std::fs::write` kullan: ilk dosyanın baytları iddiası kırmızı |
| I8 Pano başarısız olduğunda dosya yine yazılır ve `complaint` panoyu anar | yarım başarı yarım başarıdır | pano hatasında erken dön: `path` iddiası kırmızı |
| I9 Yazılamayan bir dizinle çağrıldığında `path` `None` olur, `clipboard` yine de **çağrılmıştır** ve `complaint` bunu söyler | pano dosya yüzünden kaybedilmez | `write_capture`'ı panodan önce çağır ve `?` ile dön: pano çağrıldı iddiası kırmızı |

- [ ] **Step 2:** Kırmızıyı gör, uygula, `AppPolicy`'yi `lib.rs`'te sunucuya bağla.
- [ ] **Step 3:** Tüm kapılar temiz.
- [ ] **Step 4: Commit** `feat(app): save a page delivered over the bridge`

---

### Task 5: Ayarlar, eşleştirme token'ı ve köprü durumu

**Birim:** U3. **Bağımlılık:** Görev 4.

**Files:** `apps/desktop/src-tauri/src/settings.rs`, `commands.rs`, `lib.rs`, `apps/desktop/src/settings/SettingsWindow.tsx`, `SettingsWindow.test.ts`

**Interfaces (birebir):**

```rust
// settings.rs, Settings'e eklenir
pub struct Settings {
    // ...mevcut alanlar değişmez...
    /// The token the extension has to present. Empty until the first launch
    /// that mints one.
    pub bridge_token: String,
}

/// The token in force, minting and storing one the first time.
///
/// Not a `Default`: a default is a constant, and a constant pairing token would
/// be the same secret on every installation in the world. The default is "none
/// yet", and the first launch is what mints one.
pub fn ensure_bridge_token(app: &AppHandle, settings: &mut Settings) -> Result<String, String>;
```

```rust
// commands.rs, mevcut komutlar değişmeden eklenir
#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "detail")]
pub enum BridgeStatus {
    Listening { port: u16 },
    Unavailable { reason: String },
}

// SettingsView'a eklenir: bridge_token: String, bridge_status: BridgeStatus

#[tauri::command] pub fn regenerate_bridge_token(app: AppHandle) -> Result<SettingsView, String>;
#[tauri::command] pub fn copy_bridge_token(app: AppHandle) -> Result<(), String>;
```

```ts
// SettingsWindow.tsx, shortcutNotice ile aynı desende dışa açılır
export type BridgeStatus =
  | { state: 'listening'; detail: { port: number } }
  | { state: 'unavailable'; detail: { reason: string } }

export function bridgeStatusNotice(status: BridgeStatus): string
```

**Kurallar:**

- `Settings::default().bridge_token` boş dizedir. `Default` hâlâ tek gerçek kaynaktır ve hâlâ saftır; mevcut varsayılan testleri bozulmaz.
- `lib::adopt`, ayarları yükledikten sonra `ensure_bridge_token` çağırır; token boşsa üretir, dosyaya yazar ve kullanıcının seçtiği hiçbir alanı değiştirmez.
- Ayarlar penceresine `Browser Extension` adlı bir bölüm eklenir: salt okunur token alanı, `Copy` düğmesi (`copy_bridge_token`, Rust panoya yazar; webview'a pano izni verilmez, editörle aynı gerekçe), `Regenerate` düğmesi ve durum satırı. `Regenerate`, eklentinin yeniden eşleştirilmesi gerektiğini yazıyla söyler.
- Token değişince açık oturumlar geçersiz olur: `AppPolicy::token()` her seferinde `AppState`'ten okur, önbelleğe almaz.
- `capabilities/settings.json` değişmez: yeni her şey Rust komutudur.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| T1 **Güvenlik:** `Settings::default().bridge_token` boştur | hiçbir kurulum sabit bir token paylaşmaz | `Default`'a sabit bir dize yaz: kırmızı |
| T2 `ensure_bridge_token` boş token'ı doldurur; ikinci çağrı **aynı** token'ı döner | eşleştirme her açılışta bozulmaz | `is_empty()` kontrolünü sil: ikinci çağrı farklı token üretir, kırmızı |
| T3 `Settings` serde gidiş dönüşünde `bridge_token` korunur | token diske yazılır ve geri okunur | alana `#[serde(skip)]` koy: kırmızı |
| T4 Eski, `bridgeToken` alanı olmayan bir ayar dosyası varsayılana düşmeden okunur ve token boş gelir | geriye uyum; mevcut kullanıcı ayarlarını kaybetmez | konteyner düzeyindeki `default`'u kaldır: kırmızı |
| T5 `regenerate_bridge_token` eskisinden farklı bir token üretir | yeniden üretme gerçekten yeniden üretir | aynı token'ı geri dön: kırmızı |
| T6 `bridgeStatusNotice({state:'listening',...})` port numarasını içerir; `unavailable` nedeni **ve** eklentinin çalışmayacağını söyler | kullanıcı köprünün ölü olduğunu penceresinde görür | `unavailable` kolunda `listening` metnini dön: kırmızı |

- [ ] **Step 2-4:** Kırmızıyı gör, uygula, tüm kapılar, commit `feat(app): pair the browser extension from settings`

---

### Task 6: `apps/extension`, iskelet, manifest ve build

**Birim:** U4. **Bağımlılık:** Görev 1.

**Files:** `apps/extension/{package.json,tsconfig.json,vite.config.ts,vite.content.config.ts,vitest.config.ts,options.html}`, `public/manifest.json`, `src/manifest.ts`, `src/manifest.test.ts`, `scripts/check-manifest.mjs`, `src/{background,content,options}/main.ts` (boş kabuklar)

**Interfaces (birebir):**

```ts
// src/manifest.ts
export type Manifest = {
  manifest_version: number
  version: string
  permissions: string[]
  host_permissions?: string[]
  background: { service_worker: string; type: string }
  options_ui: { page: string; open_in_tab: boolean }
}

/** Files the manifest names that the build did not produce. */
export function missingManifestFiles(manifest: Manifest, produced: string[]): string[]
```

**Kurallar:**

- `manifest.json` elle yazılır ve `public/` altındadır, yani Vite onu olduğu gibi kopyalar. İzinler tam olarak `["activeTab", "scripting", "storage", "downloads"]`.
  - `activeTab`: `captureVisibleTab` için gereken iki seçenekten dar olanı. Kullanıcı eklentinin düğmesine bastığında verilir ve o sekmeyle sınırlıdır.
  - `scripting`: content script talep üzerine enjekte edilir, `content_scripts` bildirimi yoktur. Eklenti hiçbir sayfayı kullanıcı istemeden okumaz.
  - `storage`: token ve seçenekler.
  - `downloads`: uygulama kapalıyken PNG'yi indirmeye düşürmek için. Service worker'da `URL.createObjectURL` yok, dolayısıyla indirme `chrome.downloads.download` üzerinden gider.
- `host_permissions` **yoktur**. Bu bir gizlilik iddiasıdır ve testi vardır.
- `notifications` izni **alınmaz**. Kullanıcıya bir şey söylemek gerektiğinde `chrome.action.setBadgeText('!')` ve `setTitle` kullanılır; masaüstündeki `tray::show_failure` ile aynı yaklaşım, aynı sebeple (izin gerektirmez, her kurulumda çalışır, bir sonraki eyleme kadar durur).
- İki build: `vite.config.ts` service worker ve seçenekler sayfasını ESM olarak üretir (`entryFileNames: '[name].js'`, hash yok, çünkü manifest sabit adlar bekler); `vite.content.config.ts` content script'i `formats: ['iife']` ile üretir, çünkü MV3 content script'i modül olamaz. İlki `emptyOutDir: true`, ikincisi `false`.
- **Content script `@snapdeck/protocol`'ü import etmez.** Sayfaya enjekte edilen kod yalnızca `chrome.runtime` üzerinden kendi service worker'ıyla konuşur; köprüyle konuşan tek yer service worker'dır. Bu hem şemayı content script paketine gömmekten kaçınır hem de sayfaya en az kodu koyar.
- `build` betiği sırayla: `tsc --noEmit`, ana build, content build, `node scripts/check-manifest.mjs`. Betik `missingManifestFiles`'ı çağırır ve boş olmayan bir liste için sıfırdan farklı çıkar.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| E1 **Güvenlik/gizlilik:** `manifest.json` `host_permissions` içermez | eklenti kendiliğinden hiçbir siteyi okuyamaz | `host_permissions: ["<all_urls>"]` ekle: kırmızı |
| E2 İzin listesi tam olarak dört izindir; `tabs` yoktur | `tabs` bütün sekmelerin URL'lerini okur ve gerekmiyor | `tabs` ekle: kırmızı |
| E3 `manifest_version === 3` ve `background.type === 'module'` | MV3 ve ESM service worker | `type`'ı sil: kırmızı |
| E4 `manifest.version === package.json.version` | iki sürüm ayrışamaz | birini değiştir: kırmızı |
| E5 `missingManifestFiles` `background.js` üretilmediğinde onu listeler, hepsi üretildiğinde boş dizi döner | build çıktısı manifest ile tutarlı | taramadan `options_ui.page`'i çıkar: `options.html` eksik senaryosu kırmızı |

- [ ] **Step 2-4:** Kırmızıyı gör, uygula, `pnpm --filter @snapdeck/extension build` ve `lint` temiz, commit `feat(extension): add the manifest v3 skeleton`

---

### Task 7: Content script, ölçüm, kaydırma planı ve sabit elemanlar

**Birim:** U4. **Bağımlılık:** Görev 6.

**Files:** `apps/extension/src/content/{measure.ts,plan.ts,sticky.ts,main.ts}`, `src/content/{measure,plan,sticky}.test.ts`, `vitest.config.ts` (iki proje)

`vitest.config.ts`, `packages/editor/vitest.config.ts` ile aynı kalıptadır: `node` projesi aritmetik testleri, `browser` projesi (playwright/chromium, headless) gerçek DOM ve gerçek `getComputedStyle` gerektirenleri koşar.

**Interfaces (birebir):**

```ts
// measure.ts
export type MeasurableWindow = {
  innerWidth: number
  innerHeight: number
  devicePixelRatio: number
  document: {
    documentElement: { scrollHeight: number; clientHeight: number }
    body: { scrollHeight: number }
  }
}

export type PageMetrics = {
  documentHeight: number   // CSS px
  viewportHeight: number   // CSS px
  viewportWidth: number    // CSS px
  devicePixelRatio: number
}

export function measurePage(win: MeasurableWindow): PageMetrics
```

```ts
// plan.ts
export type ScrollStep = {
  index: number
  scrollY: number     // CSS px, sayfanın bu katman için kaydırıldığı yer
  sourceTop: number   // CSS px, viewport içinde katmanın işe yarayan kısmının başladığı yer
  height: number      // CSS px, bu katmanın katkısı
  destTop: number     // CSS px, birleşimde yapıştırıldığı yer
}

export type ScrollPlan = {
  steps: ScrollStep[]
  compositeWidth: number   // CSS px
  compositeHeight: number  // CSS px
  truncated: boolean
}

export function planScroll(metrics: PageMetrics, maxPixels: number): ScrollPlan
```

```ts
// sticky.ts
export type PinnedElement = { element: HTMLElement; previousVisibility: string }

export function findPinned(root: Document): HTMLElement[]
export function hidePinned(elements: HTMLElement[]): PinnedElement[]
export function restorePinned(hidden: PinnedElement[]): void
```

**Kurallar:**

- `measurePage`, `Window` değil dar bir yapısal tip alır: böylece aritmetiği node'da test edilebilir ve gerçek `window` yine geçerli bir argümandır.
- `documentHeight`, `documentElement.scrollHeight` ile `body.scrollHeight`'ın büyüğüdür. Tek birini okumak, ikisinden biri kısa olan sayfalarda eksik yakalama üretir.
- `devicePixelRatio` sonlu ve pozitif değilse 1'e düşer. Sıfır DPR sıfır boyutlu tuval demektir.
- `planScroll` kuralları, birebir:
  - Adım sayısı `ceil(documentHeight / viewportHeight)`, en az 1.
  - `scrollY = min(index * viewportHeight, documentHeight - viewportHeight)`, sıfırın altına düşmez.
  - Son adım dışındaki her adım: `sourceTop = 0`, `height = viewportHeight`, `destTop = index * viewportHeight`.
  - Son adım: `height`, önceki adımların kapatmadığı kalan yüksekliktir; `destTop = compositeHeight - height` ve `sourceTop = viewportHeight - height`. Sayfa viewport'tan kısaysa tek adım `height = documentHeight`, `sourceTop = 0`.
  - `truncated`, `documentHeight * viewportWidth * dpr * dpr > maxPixels` olduğunda `true`'dur; o zaman `compositeHeight`, sınıra sığan en büyük tam CSS piksel yüksekliğine indirilir ve adımlar bu yüksekliğe göre üretilir (sınırın ötesine adım üretilmez).
  - Her zaman geçerli olan değişmez: `steps[i].destTop + steps[i].height === steps[i+1].destTop`, ve son adım tam `compositeHeight`'ta biter. Ne boşluk ne çakışma.
- `hidePinned` `visibility: 'hidden'` uygular, `display: 'none'` **değil**: `display: none` sayfanın akışını değiştirir ve altındaki içeriği yukarı kaydırır, yani katmanlar birbirini tutmaz. `restorePinned` her elemanın kendi önceki satır içi `visibility` değerini geri koyar, boş dize dahil.

- [ ] **Step 1: `plan.ts` testlerini yaz (node projesi)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| P1 3000 / 1000 → 3 adım, hiçbiri örtüşmez, `compositeHeight === 3000` | tam katta örtüşme yoktur | `scrollY`'yi her zaman `documentHeight - viewportHeight` ile sınırla: P1 yeşil kalır, P2 kırmızı olur |
| P2 2500 / 1000 → 3 adım; son adım `scrollY: 1500`, `sourceTop: 500`, `height: 500`, `destTop: 2000`; `height` toplamı tam 2500 | **asıl off-by-one:** son adım örtüşür ve örtüşen bant iki kez yapıştırılmaz | son adımın `sourceTop`'unu 0 yap: toplam 3000 olur, kırmızı |
| P3 800 / 1000 → tek adım, `height: 800`, `compositeHeight: 800` | viewport'tan kısa sayfa boş bant üretmez | `height`'ı `viewportHeight` bırak: kırmızı |
| P4 2000 / 1000 → 2 adım, son adımın `sourceTop`'u 0 | tam kat örtüşme uydurmaz | son adımı her zaman örtüşen kabul et: kırmızı |
| P5 `maxPixels` aşıldığında `truncated: true`, `compositeHeight` sınırın altında ve adımlar kırpılmış yüksekliğe göre üretilir | çok uzun sayfa sessizce bozuk tuval üretmez | `truncated`'ı sabit `false` bırak: kırmızı. Adımları kırpma: uzunluk iddiası kırmızı |
| P6 Her plan için `destTop + height === bir sonraki destTop` ve son adım `compositeHeight`'ta biter (P1..P5'in hepsinde) | **birleştirmenin boşluk ya da çakışma üretmediğinin kanıtı** | son adımda da `destTop = index * viewportHeight` bırak: kırmızı |
| P7 `scrollY` hiçbir adımda negatif olmaz ve `documentHeight - viewportHeight`'ı aşmaz | tarayıcının kabul etmeyeceği kaydırma istenmez | alt sınırı sil: 800/1000 durumunda `-200` çıkar, kırmızı |

- [ ] **Step 2: `measure.ts` testlerini yaz (node projesi)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| Q1 `documentElement.scrollHeight` 5000, `body.scrollHeight` 800 iken `documentHeight` 5000 olur; tersi durumda büyük olan seçilir | iki ölçünün büyüğü alınır | yalnız `body.scrollHeight` oku: kırmızı |
| Q2 `devicePixelRatio: 0` ve `NaN` girdileri 1'e düşer | sıfır DPR sıfır boyutlu tuval demektir | fallback'i sil: kırmızı |

- [ ] **Step 3: `sticky.ts` testlerini yaz (browser projesi, gerçek DOM)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| R1 `findPinned` `position: fixed` ve `position: sticky` elemanları bulur; `static`, `relative`, `absolute` olanları bulmaz | doğru elemanlar hedeflenir | `sticky`'yi listeden çıkar: kırmızı |
| R2 `hidePinned` sonrası sabit elemanın altındaki kardeş elemanın `getBoundingClientRect().top` değeri **değişmez** | `visibility: hidden` akışı bozmaz, `display: none` bozar | `display = 'none'` yaz: kırmızı |
| R3 `restorePinned`, başlangıçta satır içi `visibility: hidden` olan bir elemanı `hidden` olarak, satır içi değeri olmayanı boş dize olarak geri koyar | kullanıcının sayfası olduğu gibi bırakılır | hepsine `'visible'` yaz: birinci eleman iddiası kırmızı |

- [ ] **Step 4:** Kırmızıyı gör, uygula. `main.ts` yalnız `chrome.runtime.onMessage` üzerinden `measure`, `prepare`, `scrollTo`, `restore` komutlarını üç saf modüle bağlayan ince bir kabuktur; kendi mantığı yoktur.
- [ ] **Step 5:** Tüm kapılar temiz. Commit `feat(extension): measure the page and plan the scroll`

---

### Task 8: Service worker, yakalama döngüsü ve katman birleştirme

**Birim:** U4. **Bağımlılık:** Görev 7.

**Files:** `apps/extension/src/background/{throttle.ts,composite.ts,capture.ts}`, `src/background/{throttle,composite,capture}.test.ts`

**Interfaces (birebir):**

```ts
// throttle.ts
/**
 * Chrome refuses more than MAX_CAPTURE_VISIBLE_TAB_CALLS_PER_SECOND (2) calls
 * to captureVisibleTab in any second, and the refusal is an error rather than
 * a queue. 550 rather than 500 because the quota is measured against a moving
 * window and a call landing exactly on the boundary is the one that fails.
 */
export const CAPTURE_INTERVAL_MS = 550

export function createThrottle(
  intervalMs: number,
  now: () => number,
  sleep: (ms: number) => Promise<void>,
): <T>(work: () => Promise<T>) => Promise<T>
```

```ts
// composite.ts
import type { ScrollPlan, ScrollStep } from '../content/plan'

export type CapturedLayer = { bitmap: ImageBitmap; step: ScrollStep }

/**
 * Pastes each layer where its step says, in device pixels.
 *
 * The plan is in CSS pixels and the bitmaps are in device pixels, so every
 * rectangle is multiplied by the ratio on the way in. Source and destination
 * rectangles are always the same size: nothing here resamples, which is what
 * keeps text as sharp as the screen had it.
 */
export function compositeLayers(
  layers: CapturedLayer[],
  plan: ScrollPlan,
  devicePixelRatio: number,
): OffscreenCanvas

export function compositeToPng(
  layers: CapturedLayer[],
  plan: ScrollPlan,
  devicePixelRatio: number,
): Promise<Blob>
```

```ts
// capture.ts
export type CaptureDeps = {
  measure(): Promise<PageMetrics>
  /** Hides every sticky and fixed element, or restores them. */
  setPinnedHidden(hidden: boolean): Promise<void>
  scrollTo(y: number): Promise<void>
  /** Waits for lazy-loaded content to arrive. */
  settle(): Promise<void>
  captureVisible(): Promise<ImageBitmap>
  restoreScroll(): Promise<void>
}

export type FullPageCapture = { blob: Blob; plan: ScrollPlan; metrics: PageMetrics }

export function captureFullPage(deps: CaptureDeps, maxPixels: number): Promise<FullPageCapture>
```

**Kurallar:**

- `captureFullPage` sırası, birebir: ölç, planla, ilk adıma kaydır, `settle`, **ilk katmanı yakala**, ardından `setPinnedHidden(true)`, sonra kalan her adım için kaydır, `settle`, yakala. Sabit elemanlar **ilk katmandan sonra** gizlenir; önce gizlenirse sayfanın kendi başlığı hiçbir katmanda görünmez (spec 7.1 madde 2).
- Yakalama çağrıları `createThrottle(CAPTURE_INTERVAL_MS, …)` üzerinden geçer.
- `finally` bloğunda her yoldan `setPinnedHidden(false)` ve `restoreScroll()` çağrılır. Bir yakalama hatası kullanıcının sayfasını gizlenmiş başlıklarla ve kaydırılmış halde bırakmaz.
- `compositeLayers`: tuval `plan.compositeWidth * dpr` x `plan.compositeHeight * dpr` boyutundadır. Her katman `drawImage(bitmap, 0, step.sourceTop * dpr, w * dpr, step.height * dpr, 0, step.destTop * dpr, w * dpr, step.height * dpr)` ile çizilir. Dokuz argümanlı biçim zorunludur; beş argümanlı biçim hedefe ölçekler ve bulanıklık üretir.
- `compositeToPng`, `convertToBlob({ type: 'image/png' })` kullanır.

- [ ] **Step 1: `throttle.ts` testlerini yaz (node projesi, sahte saat)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| H1 Art arda üç iş; ilki hemen koşar, toplam uyku `2 * intervalMs`'tir | kota gerçekten uygulanır, gerçek zaman beklenmez | `sleep` çağrısını kaldır: toplam 0, kırmızı |
| H2 İki iş arasında `intervalMs`'ten uzun bir süre geçtiyse ikincisi beklemez | gereksiz bekleme yok | geçen süreyi hesaba katmadan her seferinde uyu: kırmızı |
| H3 `1000 / CAPTURE_INTERVAL_MS < 2` | Chrome'un saniyede 2 kotasının altında kalınır | `CAPTURE_INTERVAL_MS`'i 400 yap: kırmızı |

- [ ] **Step 2: `composite.ts` testlerini yaz (browser projesi, gerçek `OffscreenCanvas`)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| K1 DPR 2, `compositeWidth: 400`, `compositeHeight: 600` verildiğinde tuval 800x1200 device px olur | **DPR düzeltmesi**, spec 7.1 madde 4 | tuvali CSS px'te oluştur: kırmızı |
| K2 İki düz renkli katmanda, `destTop * dpr - 1` satırı birinci rengi, `destTop * dpr + 1` satırı ikinci rengi verir | her katman kendi yerine yapıştırılır | `step.destTop` yerine `index * viewportHeight` kullan: P2'nin örtüşen son adımında sınır yanlış satıra düşer, kırmızı |
| K3 Üç renkli sentetik katmanla, son katmanın örtüşen üst bandı **yeniden yapıştırılmaz**: önceki katmanın alt bandının rengi korunur | `sourceTop` gerçekten kaynak dikdörtgenine gider | `sourceTop`'u `drawImage`'ın kaynak y'sine verme: kırmızı |
| K4 1 piksel genişliğinde dikey çizgiler taşıyan bir katman, birleşimde hâlâ 1 piksel genişliğinde ve aynı renktedir (ara ton yok) | hiçbir yerde ölçekleme yok | 9 argümanlı `drawImage`'ı 5 argümanlıya çevir: yumuşama başlar, kırmızı |
| K5 `compositeToPng` çıktısı `image/png` tipinde ve boş olmayan bir `Blob`'dur; `createImageBitmap` ile geri okunduğunda boyutu K1'in tuvaliyle aynıdır | çıktı gerçekten bir PNG | `type: 'image/jpeg'` yaz: kırmızı |

- [ ] **Step 3: `capture.ts` testlerini yaz (node projesi, sahte `CaptureDeps`)**

Sahte `deps` her çağrıyı sırasıyla bir diziye kaydeder; `captureVisible` 1x1 sahte bitmap döner.

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| C1 `setPinnedHidden(true)` çağrısı **ilk** `captureVisible`'dan **sonra** gelir | spec 7.1 madde 2: ilk katmanda sabit elemanlar görünür | gizlemeyi döngünün başına al: kırmızı, ve o zaman sayfanın başlığı hiçbir katmanda olmaz |
| C2 `captureVisible` üçüncü çağrıda fırlattığında `setPinnedHidden(false)` **ve** `restoreScroll` yine çağrılır | kullanıcının sayfası bozuk bırakılmaz | `finally` bloğunu sil: kırmızı |
| C3 Her `captureVisible`'dan önce tam bir `settle` çağrısı vardır, adım sayısı kadar | lazy-load içeriğe bekleme verilir | `settle`'ı atla: sayı iddiası kırmızı |
| C4 Adım sayısı `planScroll`'un ürettiği adım sayısıyla aynıdır ve `scrollTo` her adımın kendi `scrollY`'siyle çağrılır | plan gerçekten uygulanır | `scrollTo`'ya `index * viewportHeight` gönder: son adımda değer farklı, kırmızı |
| C5 Dönen `plan` ve `metrics`, çağırana olduğu gibi verilir (köprü mesajının `truncated` ve boyut alanları buradan gelir) | üst katman uydurmaz | `truncated`'ı sabit `false` döndür: kırmızı |

- [ ] **Step 4:** Kırmızıyı gör, uygula. Tüm kapılar temiz. Commit `feat(extension): capture and stitch the whole page`

---

### Task 9: Köprü istemcisi, seçenekler sayfası ve indirme fallback'i

**Birim:** U4. **Bağımlılık:** Görev 8.

**Files:** `apps/extension/src/background/{bridge.ts,fallback.ts,main.ts}`, `src/options/{main.ts,token.ts}`, `options.html`, `src/background/{bridge,fallback}.test.ts`, `src/options/token.test.ts`

**Interfaces (birebir):**

```ts
// bridge.ts
import type { ErrorCode, FullPage } from '@snapdeck/protocol'

export type BridgeSocket = {
  send(text: string): void
  close(): void
  onMessage(handler: (text: string) => void): void
  onClose(handler: () => void): void
}

export type SendOutcome =
  | { ok: true; savedPath: string | null }
  | { ok: false; code: ErrorCode | 'unreachable'; message: string }

/**
 * Opens a session, hands over one page, and closes.
 *
 * Never throws: the caller's answer to every failure is the same, fall back to
 * a download, and a thrown error would make that a `catch` in three places.
 */
export function sendFullPage(
  open: () => Promise<BridgeSocket>,
  token: string,
  page: Omit<FullPage, 'type'>,
  timeoutMs: number,
): Promise<SendOutcome>
```

```ts
// fallback.ts
export function fallbackFilename(url: string, at: Date): string

export function downloadInstead(
  blob: Blob,
  filename: string,
  download: (options: { url: string; filename: string }) => Promise<number>,
): Promise<void>

/** The badge and title the action wears when the desktop app is not there. */
export function unreachableBadge(): { text: string; title: string }
```

```ts
// src/options/token.ts
export function tokenLooksValid(token: string): boolean
```

**Kurallar:**

- `sendFullPage` sırası: aç, `hello` gönder (`protocolVersion` `PROTOCOL_VERSION` sabitinden okunur, elle yazılmaz), `ready`'yi bekle, `fullPage` gönder, `accepted` veya `error` bekle, kapat.
- Gelen her çerçeve `parseServerMessage`'dan geçer. Ayrıştırılamayan bir çerçeve `{ ok: false, code: 'malformedMessage' }` üretir ve **istemci soketi kendisi kapatır**. Güven sınırı iki yönlüdür: masaüstü eklentiye güvenmediği gibi eklenti de masaüstünden geleni doğrular.
- `timeoutMs` içinde beklenen mesaj gelmezse soket kapatılır ve `unreachable` döner.
- `open` reddederse ya da soket `accepted`'tan önce kapanırsa `unreachable` döner; fırlatmaz.
- Sunucudan gelen `error`'un `code`'u **olduğu gibi** taşınır. `unsupportedProtocolVersion`, `unreachable` diye raporlanmaz: kullanıcıya söylenecek şey farklıdır (spec 9).
- Uygulama erişilemezse `main.ts` `downloadInstead` çağırır ve `unreachableBadge()` ile eylem simgesine `!` koyar. Bildirim izni alınmaz.
- Seçenekler sayfası düz TypeScript ve DOM'dur. Token `chrome.storage.local`'da durur. `Test connection` düğmesi yalnız `hello`/`ready` yapar ve sonucu bir satırda gösterir.

- [ ] **Step 1: Testleri yaz (sahte `BridgeSocket`)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| G1 Mutlu yol: `hello` gönderilir, **`ready` gelmeden** `fullPage` gönderilmez, `accepted` `{ok:true, savedPath}` üretir | el sıkışma sırası | `ready`'yi beklemeden `fullPage` gönder: sahte soketin kaydettiği sıra iddiası kırmızı |
| G2 Gönderilen `hello`'nun `protocolVersion`'ı `PROTOCOL_VERSION`'a eşittir | sürüm tek kaynaktan | elle `2` yaz: kırmızı |
| G3 Sunucu `error{unsupportedProtocolVersion}` döndüğünde sonuç o kodu taşır ve mesaj sürüm uyuşmazlığını söyler | spec 9 satırı kullanıcıya doğru ulaşır | her hatayı `unreachable` diye raporla: kırmızı |
| G4 `open` reddettiğinde `{ok:false, code:'unreachable'}` döner ve **fırlatmaz** | uygulama kapalıysa indirmeye düşülür | hatayı yeniden fırlat: kırmızı |
| G5 `ready` hiç gelmezse `timeoutMs` sonunda `unreachable` döner **ve** `close` çağrılmıştır | sızan soket yok | `close` çağrısını sil: kırmızı |
| G6 **Güvenlik:** şema dışı bir sunucu çerçevesi (`{"type":"ready"}` eksik alanlarla) `malformedMessage` üretir ve istemci soketi kapatır | güven sınırı iki yönlü | `parseServerMessage` yerine düz `JSON.parse` kullan: kırmızı |
| G7 `accepted` içinde `savedPath: null` geldiğinde `{ok:true, savedPath:null}` döner | yalnız panoya yazılmış bir yakalama başarıdır | `null`'ı hata say: kırmızı |
| F1 `fallbackFilename('https://a.example/x?y=1', tarih)` ana bilgisayarı ve tarihi taşır; `/`, `:`, `?` dosya adında kalmaz | geçerli bir dosya adı üretilir | temizlemeyi sil: kırmızı |
| F2 `fallbackFilename('about:blank', tarih)` yine boş olmayan geçerli bir ad üretir | ayrıştırılamayan URL çökme sebebi değil | `new URL`'i try/catch dışına al: kırmızı |
| F3 `downloadInstead` `download`'a `data:image/png;base64,…` biçiminde bir URL verir | service worker'da `createObjectURL` yok | `URL.createObjectURL` kullan: sahte ortamda tanımsız, kırmızı |
| F4 `unreachableBadge().text` `'!'`'tir ve `title` Snapdeck'in açık olmadığını söyler | kullanıcı sessiz kalmaz | `text`'i boş dize yap: kırmızı |
| O1 `tokenLooksValid` 64 haneli küçük harf hex'i kabul eder; 63 hane, büyük harf ve boşluk taşıyanı reddeder | yanlış yapıştırılmış token daha bağlanmadan yakalanır | uzunluk kontrolünü sil: kırmızı |

- [ ] **Step 2-4:** Kırmızıyı gör, uygula, `main.ts`'i bağla, tüm kapılar temiz, commit `feat(extension): hand the page to the desktop app`

---

### Task 10: `crates/stitch`, hizalama

**Birim:** U2. **Bağımlılık:** yok (Görev 1 ile paralel koşabilir).

**Files:** `crates/stitch/Cargo.toml`, `crates/stitch/src/{lib.rs,error.rs,align.rs}`, kök `Cargo.toml` (yalnız `members` satırı)

**Interfaces (birebir):**

```rust
// error.rs, crates/capture/src/error.rs desenini izler
#[derive(Debug, thiserror::Error, Serialize, Clone, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum StitchError {
    #[error("there is nothing to stitch")]
    NoFrames,
    #[error("the frames disagree about their shape: {0}")]
    MismatchedFrames(String),
    #[error("no overlap was found between frame {index} and the one before it")]
    NoOverlap { index: usize },
    #[error("the stitched picture would hold {pixels} pixels, past the limit of {limit}")]
    TooLarge { pixels: u64, limit: u64 },
}
```

```rust
// align.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis { Vertical }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollHint {
    pub axis: ScrollAxis,
    /// The smallest displacement worth considering, in pixels. Below it two
    /// frames are the same picture and the scroll did nothing.
    pub min_shift: u32,
    /// The correlation a match has to reach to count, 0.0 to 1.0.
    pub min_score: f32,
}

impl Default for ScrollHint {
    /// Vertical, 8 pixels, 0.90.
    fn default() -> Self;
}

/// One row reduced to the numbers a vertical alignment needs.
///
/// A row rather than a pixel, because the displacement being searched for is
/// one-dimensional: a vertical scroll moves every row by the same amount, so
/// the whole width collapses into one number per row without losing the
/// quantity being measured.
pub struct RowSignatures { /* private */ }

impl RowSignatures {
    /// Signatures for a tightly packed RGBA8 image.
    pub fn of(rgba: &[u8], width: u32, height: u32) -> Result<Self, StitchError>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alignment {
    /// Rows at the top of `next` that repeat the bottom of `previous`.
    pub overlap: u32,
    /// Zero-normalised correlation of the overlapping rows, 0.0 to 1.0.
    pub score: f32,
}

/// Finds how far `next` scrolled past `previous`.
pub fn align(
    previous: &RowSignatures,
    next: &RowSignatures,
    hint: ScrollHint,
) -> Result<Alignment, StitchError>;
```

**Kurallar ve algoritma kararı:**

- Tasarım dokümanı bu adımı "faz korelasyonu" diye adlandırıyor. Faz korelasyonunun burada kazandırdığı şey, genlikle normalizasyonun parlaklık ve kontrast farkını yok saymasıdır. Aranan yer değiştirme tek boyutlu ve kare yüksekliğiyle sınırlı olduğu için aynı normalizasyon uzamsal alanda, satır imzaları üzerinde sıfır ortalamalı normalize çapraz korelasyonla (ZNCC) elde edilir: 2000 satırlık bir kare için bütün kaydırmaların taranması birkaç milyon işlemdir. Bu yüzden `rustfft` taşınmaz. Karar A3 testiyle kanıtlanır; gerçek yakalamalarda yetmediği ölçülürse FFT'ye geçmek `align`'ın imzasını değiştirmez.
- `align`, `overlap` olarak **`next`'in tepesinde tekrarlanan satır sayısını** döner, kaydırma miktarını değil. İkisi birbirinin tümleyenidir ve karıştırmak çıktıyı ters yönde bozar.
- Aday örtüşmeler `hint.min_shift`'ten kare yüksekliğine kadar taranır; en yüksek skorlu aday seçilir. Skor `hint.min_score`'un altındaysa `NoOverlap` döner, sessizce sıfır dönmez.
- `RowSignatures::of`, tampon `width * height * 4`'ten kısaysa `MismatchedFrames` döner, indekslemeye girmez.
- Crate `snapdeck-capture` dışında bir bağımlılık taşımaz (artı `serde` ve `thiserror`, ikisi de çalışma alanından). Ekran, pencere ve dosya sistemi bilmez.

- [ ] **Step 1: Testleri yaz (hepsi sentetik ve deterministik)**

Yardımcı: satır numarasına göre değişen, tekrarsız bir desen üreten `fn synthetic(width: u32, height: u32, offset: u32) -> Vec<u8>`.

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| A1 200 satırlık bir görüntüden 60 satır kaydırılmış iki kare: `overlap == 140`, `score > 0.99` | temel hizalama | `overlap`'i `height - shift` yerine `shift` döndür: kırmızı |
| A2 Tamamen ilgisiz iki görüntü `NoOverlap` döner | eşik gerçekten kapı | `min_score` eşiğini kaldır: en iyi ama kötü eşleşme kabul edilir, kırmızı |
| A3 İkinci karenin her kanalı sabit bir miktar artırılmış olsa bile `overlap` A1'inkiyle aynı kalır | **normalizasyon iddiasının kanıtı**, faz korelasyonundan beklenen özellik | ZNCC'den ortalama çıkarmayı sil: skor düşer, kırmızı |
| A4 Her 20 satırda bir tekrar eden bant desenli görüntüde `min_shift: 8` ile "hiç kaydırılmamış" cevabı (`overlap == height`) üretilmez | küçük ve sahte eşleşmeler elenir | `min_shift`'i 0 yap: kırmızı |
| A5 Boyutları farklı iki kare `MismatchedFrames` döner | biçimsizlik sessiz yanlış cevap üretmez | kontrolü sil: kırmızı (ya da panik) |
| A6 `RowSignatures::of` tampon kısa olduğunda hata döner, **panik atmaz** | kare verisi doğrulanır, güvenilmez | uzunluk kontrolünü sil: test panikle kırmızı |
| A7 Örtüşmenin tam kare yüksekliği kadar olduğu durum (iki özdeş kare) `min_shift` yüzünden `NoOverlap` döner | kaydırma olmadıysa birleştirme yapılmaz | özdeş kareyi geçerli say: kırmızı |

- [ ] **Step 2-4:** Kırmızıyı gör, uygula, `cargo test -p snapdeck-stitch` ve tüm Rust kapıları temiz, commit `feat(stitch): find the overlap between scrolled frames`

---

### Task 11: `crates/stitch`, birleştirme ve `stitch()`

**Birim:** U2. **Bağımlılık:** Görev 10.

**Files:** `crates/stitch/src/{compose.rs,lib.rs}`, testler

**Interfaces (birebir):**

```rust
/// A stitched picture: tightly packed RGBA8, and the scale it was measured at.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
}

/// The most pixels a stitched picture may hold.
pub const MAX_STITCHED_PIXELS: u64 = 64_000_000;

/// One frame reduced to what composition needs.
pub struct Tile<'a> { pub rgba: &'a [u8], pub width: u32, pub height: u32 }

/// Pastes tiles into one picture, dropping from each tile the `overlaps[i]`
/// rows it repeats from the one before it.
///
/// `overlaps` holds one fewer entry than `tiles`.
///
/// Split out from `stitch` for the reason `lib::adopt_shortcuts` is: the
/// arithmetic worth testing is where each frame lands, and building thirty
/// frames to test one paste is a slower way of asking the same question.
pub fn compose(tiles: &[Tile<'_>], overlaps: &[u32]) -> Result<Image, StitchError>;

/// Joins a sequence of overlapping frames into one picture.
///
/// The frames must be in scroll order and must all agree about their size and
/// their scale. Nothing here reads a screen, a window or a file: the input is
/// pixels and the output is pixels, which is what lets every case be built by
/// hand.
pub fn stitch(frames: &[Frame], hint: ScrollHint) -> Result<Image, StitchError>;
```

**Kurallar:**

- `stitch` sırası: boşsa `NoFrames`; tek kare ise o karenin kendisi (hizalama denenmez); bütün karelerin `width`, `height` ve `scale_factor` değerleri aynı değilse `MismatchedFrames`; toplam yükseklik `MAX_STITCHED_PIXELS`'i aşacaksa `TooLarge` (ayırmadan **önce**); sonra ardışık çiftler `align` ile hizalanır ve `compose` çağrılır.
- `NoOverlap`, hangi karede olduğunu taşır: kullanıcıya "eksik olabilir" demenin dayanağı budur (spec 9).
- `Image.scale_factor` ilk karenin ölçeğidir ve bütün kareler onunla aynı olmak zorundadır. Karışık DPI sessizce yanlış ölçekli bir görüntü üretmez.
- `compose`, `overlaps` uzunluğu `tiles.len() - 1` değilse ve bir örtüşme kendi karesinin yüksekliğini aşıyorsa hata döner.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| N1 Üçü de 100 satırlık kareler, örtüşmeler 40 ve 40 → sonuç 220 satır ve her satır kaynaktaki karşılığıyla **bayt bayt** aynı | temel yapıştırma | örtüşen satırları atmak yerine yapıştır: yükseklik 300, kırmızı |
| N2 `overlaps` uzunluğu yanlışsa hata döner, indekslemeye girmez | biçimsiz girdi panik değil hata | kontrolü sil: panik, kırmızı |
| N3 Bir örtüşme kendi karesinin yüksekliğinden büyükse hata döner | negatif katkı imkânsız | kontrolü sil: taşma veya panik, kırmızı |
| N4 `stitch` tek kareyle o karenin kendisini döner ve `align` çağrılmaz | tek kare hizalanacak bir şey değil | tek kare için de `align` çağır: kırmızı |
| N5 `stitch` boş dilimle `NoFrames` döner | boş girdi panik değil | `is_empty` kontrolünü sil: panik, kırmızı |
| N6 **Uçtan uca determinizm:** bilinen kaydırmalarla (60, 45, 70) bir kaynaktan kesilmiş dört kare, `stitch` ile kaynağın kendisine **bayt bayt eşit** olacak şekilde geri kurulur | hizalama ve yapıştırma birlikte doğru | `align`'ın `overlap`'ini bir satır kaydır: çıktı bir satır kayar, karşılaştırma kırmızı |
| N7 `scale_factor`'leri farklı kareler `MismatchedFrames` döner | spec 9'un karışık DPI satırı | kontrolü sil: sessizce yanlış ölçekli görüntü, kırmızı |
| N8 Hem çok büyük hem boyutları uyuşmayan bir girdi `TooLarge` döner (`MismatchedFrames` değil) | boyut kontrolü ayırmadan önce | iki kontrolün sırasını değiştir: kırmızı |
| N9 Ortadaki bir kare öncekiyle örtüşmüyorsa `NoOverlap { index: 2 }` döner ve **indeks doğrudur** | kullanıcıya nerede koptuğu söylenebilir | `index`'i sabit 0 yaz: kırmızı |

- [ ] **Step 2-4:** Kırmızıyı gör, uygula, tüm Rust kapıları temiz, commit `feat(stitch): join overlapping frames into one picture`

---

### Task 12: Masaüstü fallback sürücüsü ve "deneysel" etiket

**Birim:** U3. **Bağımlılık:** Görev 5 ve Görev 11.

> **Bu görev bir ürün kararına bağlıdır.** Sentetik kaydırma olayı üretmek (`CGEventPost`) macOS'ta **Accessibility (TCC) izni** ister. Bu, bir ekran görüntüsü uygulamasının kullanıcıdan istediği **ikinci** sistem izni olur ve teknik değil ürün kararıdır. Karar "şimdi değil" ise bu görev düşer: `crates/stitch` test edilmiş ama bağlanmamış olarak kalır, tepsi maddesi eklenmez, Görev 1-11 ve 13 etkilenmez.

**Files:** `apps/desktop/src-tauri/src/fullpage.rs`, `tray.rs`, `commands.rs`, `Cargo.toml`

**Interfaces (birebir):**

```rust
/// Scrolls the frontmost window, so the capture loop can be tested without a
/// window server.
pub trait ScrollDriver {
    /// Scrolls by `lines`, positive meaning downwards.
    fn scroll(&self, lines: i32) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// The content stopped moving. The ordinary end.
    ContentSettled,
    /// `max_steps` was reached, so the picture is probably short.
    StepLimit,
    /// A capture failed. What was already collected is still worth stitching.
    CaptureFailed(String),
}

pub struct FullPageRun { pub frames: Vec<Frame>, pub stopped: StopReason }

/// Captures a window repeatedly, scrolling between shots.
///
/// Never fails: a partial run is a partial picture, and spec 9 says a partial
/// picture is offered with a warning rather than thrown away.
pub fn run<C, D>(capture: C, driver: &D, max_steps: usize) -> FullPageRun
where
    C: FnMut() -> Result<Frame, String>,
    D: ScrollDriver;

/// The tray label, which has to say what this is.
pub const FULLPAGE_MENU_LABEL: &str = "Capture Scrolling Window (Experimental)";

/// Whether macOS will let this process post a synthetic scroll.
pub fn accessibility_is_granted() -> bool;
```

**Kurallar:**

- `run` sırası: yakala, kaydır, yakala, ... İlk kareden **önce** kaydırılmaz; kaydırılırsa sayfanın tepesi kaybolur.
- Yeni kare bir öncekiyle bayt bayt aynıysa içerik durmuştur: döngü biter, `ContentSettled`. Sonsuz kaydıran içerik için `max_steps` sınırdır.
- Yakalama hata verirse o ana kadarki kareler **korunur** ve `CaptureFailed` ile birlikte döner. `?` ile erken dönüş yoktur.
- Sürücü, `core-graphics`'in `CGEvent` scroll wheel olayıyla yazılır. `accessibility_is_granted` `false` ise yakalama hiç başlamaz: `commands::permission_state` ile aynı desende yönlendiren bir mesaj ve `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility` derin linki verilir. Sessizce boş kare üretilmez (spec 9).
- Tepsiye `FULLPAGE_MENU_LABEL` ile bir madde eklenir. Etiket "Experimental" kelimesini taşımak zorundadır.
- Sonuç `stitch` ile birleşir ve `bridge::intake::deliver` ile aynı yoldan kaydedilir; `NoOverlap` durumunda kısmi sonuç sunulur ve `report_failure` ile "eksik olabilir" denir.

- [ ] **Step 1: Testleri yaz (sahte `capture` ve sahte `ScrollDriver`)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| D1 İçerik dördüncü karede duruyorsa döngü 4 karede biter ve `ContentSettled` döner | durma kuralı çalışır | "aynı kare" kontrolünü sil: `max_steps` kadar kare toplanır, kırmızı |
| D2 Hiç durmayan içerikte `max_steps: 5` verildiğinde 5 karede durur ve `StepLimit` döner | sonsuz döngü yok | sınırı sil: test zaman aşımına uğrar, kırmızı |
| D3 Kareler arasında tam bir `scroll` çağrısı vardır ve **ilk kareden önce hiç** yoktur | sayfanın tepesi kaybolmaz | ilk kareden önce de kaydır: sayı ve sıra iddiası kırmızı |
| D4 `capture` üçüncü çağrıda hata verdiğinde ilk iki kare korunur ve `stopped` `CaptureFailed` olur | kısmi sonuç atılmaz | `?` ile erken dön: `frames` boş, kırmızı |
| D5 `scroll` hata verdiğinde de o ana kadarki kareler korunur ve döngü biter | sürücü hatası da kısmi sonuçtur | sürücü hatasını yoksay: sonsuz döngü, kırmızı |
| D6 `FULLPAGE_MENU_LABEL` "Experimental" içerir **ve** `include_str!("tray.rs")` menüyü bu sabitle kurar (elle yazılmış bir metinle değil) | kullanıcı deneysel olduğunu görür | etiketten "Experimental"ı çıkar veya `tray.rs`'e düz metin yaz: kırmızı |

- [ ] **Step 2-4:** Kırmızıyı gör, uygula, tüm kapılar temiz, commit `feat(app): capture a scrolling window, experimentally`

---

### Task 13: CI, uçtan uca doğrulama ve belgeler

**Birim:** U5. **Bağımlılık:** Görev 9, Görev 11 (ve yapıldıysa Görev 12).

**Files:** `.github/workflows/ci.yml`, `README.md`, `docs/EXTENSION.md`, kök `package.json` (`prepare` betiği)

**CI değişiklikleri:**

- `rust` işi: `cargo test --workspace` `crates/stitch`'i üye olduğu andan itibaren zaten kapsar; ek adım yok.
- `web` işi: `pnpm --filter @snapdeck/extension exec playwright install --with-deps chromium` eklenir (eklentinin `browser` projesi Chromium'a ihtiyaç duyar), ve `pnpm build`'den sonra `pnpm --filter @snapdeck/extension build` adımı eklenir. Bu adım `check-manifest.mjs`'i de koşturur.
- Kök `package.json`'un `prepare` betiği, editörün yanında eklentinin Chromium'unu da kurar.

- [ ] **Uçtan uca doğrulama (elle, ölçerek, göz kararı değil)**

Kurulum:
- [ ] `pnpm --filter @snapdeck/extension build`; Chrome'da `chrome://extensions` > Developer mode > Load unpacked > `apps/extension/dist`.
- [ ] Snapdeck **paketli `.app` ile** çalıştırılır. Ölçüm debug derlemesinde yapılmaz; zamanlamalar ve pencere davranışı yanıltıcıdır (Plan 4'ün kuralı burada da geçerli).
- [ ] Ayarlar > Browser Extension'dan token kopyalanır, eklentinin seçenekler sayfasına yapıştırılır, `Test connection` başarılı.

Fixture: sabit bir üst başlık, lazy-load görseller, 8-10 viewport uzunluğunda içerik, her 100 CSS px'te bir artan numara basan bir şerit, ve 1 piksellik dikey çizgiler taşıyan bir blok.

- [ ] **Kanıt 1:** Eklenti düğmesine basılır; masaüstünde bir editör penceresi açılır ve başlığı kaydedilen dosyanın adıdır.
- [ ] **Kanıt 2:** Kaydedilen PNG'nin piksel yüksekliği `sips -g pixelHeight` ile okunur ve `documentHeight * devicePixelRatio` ile karşılaştırılır.
- [ ] **Kanıt 3:** Sabit başlık çıktıda **yalnız bir kez** görünür. Ölçüm: başlığın bilinen bir renk imzası çıktıda tek bir y bandında bulunur.
- [ ] **Kanıt 4:** Katman sınırlarında yinelenen bant yoktur. Ölçüm: numara şeridi çıktıda 1'den N'e kadar, atlamasız ve tekrarsız okunur. P2 ve P6 testlerinin gerçek sayfadaki karşılığıdır.
- [ ] **Kanıt 5:** 1 piksellik dikey çizgiler çıktıda hâlâ 1 piksel ve ara tonsuzdur (K4'ün gerçek karşılığı).
- [ ] **Kanıt 6 (uygulama kapalı):** Snapdeck kapatılır, aynı düğmeye basılır. PNG indirilenlere düşer ve eklenti simgesinde `!` rozeti ile "Snapdeck açık değil" başlığı belirir.
- [ ] **Kanıt 7 (güvenlik):** Sıradan bir web sayfasının konsolundan `new WebSocket('ws://127.0.0.1:51837')` denenir. El sıkışma reddedilir ve masaüstünde hiçbir şey olmaz. Ağdaki başka bir makineden aynı port denenir: bağlantı kurulmaz.
- [ ] **Kanıt 8 (sürüm uyuşmazlığı):** Eklentinin `PROTOCOL_VERSION`'ı elle 2 yapılıp yeniden derlenir. Eklenti sürüm uyuşmazlığını söyler, masaüstü bağlantıyı kapatır, ve kullanıcı "yetki hatası" görmez.
- [ ] **Kanıt 9 (yanlış token):** Seçeneklerdeki token'ın bir karakteri değiştirilir. Eklenti yetki hatası söyler ve indirmeye düşer.
- [ ] **Kanıt 10 (Görev 12 yapıldıysa):** Kaydırılabilir bir uygulamada tepsiden `Capture Scrolling Window (Experimental)` çalıştırılır; çıktı birleşir, örtüşme bulunamayan bir noktada kullanıcı "eksik olabilir" uyarısı alır.

Belgeler:
- [ ] `docs/EXTENSION.md`: kurulum, eşleştirme, hangi izinlerin neden istendiği (ve `host_permissions`'ın **neden yok** olduğu), köprünün ne yaptığı ve yapmadığı.
- [ ] **Bilinen sınır, ölçülmüş bir örnekle:** lazy-load görseller ve sanal listeler eksik çıkabilir. Fixture'da hangi elemanın eksik çıktığı ve `settle` beklemesinin ne kadar yardım ettiği yazılır. Spec 7.1 "garanti verilmez" diyor; burada o cümlenin ölçümü durur.
- [ ] `README.md`: Full page bölümü, eklenti kurulumu, köprünün loopback ve token güvenliği, `crates/stitch`'in deneysel olduğu.
- [ ] Commit `docs: document full page capture`

---

## Görev sırası ve paralellik

Aynı sahiplik biriminde aynı anda yalnız bir ajan çalışır. Dalgalar arası geçiş, önceki dalganın **tamamının** yeşil olmasıyla olur.

| Dalga | Paralel koşabilenler | Birimler |
|---|---|---|
| 0 | **Görev 1** ∥ **Görev 10** | U1, U2 |
| 1 | **Görev 2** ∥ **Görev 6** ∥ **Görev 11** | U3, U4, U2 |
| 2 | **Görev 3** ∥ **Görev 7** | U3, U4 |
| 3 | **Görev 4** ∥ **Görev 8** | U3, U4 |
| 4 | **Görev 5** ∥ **Görev 9** | U3, U4 |
| 5 | **Görev 12** (ürün kararına bağlı) | U3 |
| 6 | **Görev 13** | U5 |

Bağımlılık gerekçeleri:
- Görev 2, Görev 1'in `limits.ts` ve `messages.ts` dosyalarını okuyan çapraz dil testlerini yazar.
- Görev 6, `package.json`'da `@snapdeck/protocol`'ü çalışma alanı bağımlılığı olarak bildirir.
- Görev 8, Görev 7'nin `ScrollPlan` tipini tüketir.
- Görev 12, hem Görev 11'in `stitch()`'ine hem Görev 5'in ayarlarına dokunur ve U3'te sıraya girer.
- Görev 13, eklentinin ve stitch'in bitmiş olmasını ister.

Her ajana verilecek brief, o görevin **Files** satırını sahiplik listesi olarak taşır. Listede olmayan bir dosyaya dokunulmaz; gerekliyse görev durur ve söylenir.

---

## Açık kararlar

1. **Görev 12 ve Accessibility izni.** Sentetik kaydırma, macOS'ta ikinci bir TCC izni ister. Ürün kararı: istenecek mi, yoksa masaüstü fallback bu sürümde bağlanmadan mı kalacak?
2. **Sabit port 51837.** Köprü sabit bir portta dinler, çünkü tek kurulum adımının "token'ı yapıştır" olması buna bağlı. Port doluysa köprü açılmaz ve kullanıcıya söylenir; alternatif port keşfi bu planda yok. Onaylanıyor mu?
3. **Eklenti kimliği sabitlenmiyor.** `Origin` kontrolü şema ve kimlik biçimiyle sınırlı, belirli bir kimlikle değil. Eklenti Chrome Web Store'a yayınlanacaksa yayın kimliği sabitlenebilir ve kural daralır; bu karar yayın kararına bağlı.

---

## Öneriler (plan dışı, bu planda uygulanmayacak)

1. **Token macOS Keychain'de durabilir.** Bugün ayarlar JSON'ında düz metin. Loopback bir eşleştirme sırrı için kabul edilebilir (dosya kullanıcının kendi dizininde), ama Keychain doğru yer olurdu.
2. **Gerçek FFT tabanlı faz korelasyonu.** `align` ZNCC kullanıyor ve A3 normalizasyon iddiasını kanıtlıyor. Gerçek yakalamalarda sıkıştırma gürültüsü veya kısmi yeniden çizim yüzünden yetmediği ölçülürse `rustfft` ile geçiş, `align`'ın imzasını değiştirmeden yapılabilir.
3. **Sticky şerit tespiti `crates/stitch`'te.** Bugün spec "sabit başlıkları ayırt edemez" diyor. `align`'ın skorunun ilk N satırda düşük çıkmasını ölçerek sabit bir şerit tahmin edilebilir ve o şerit yalnız ilk karede tutulabilir; eklenti yolunun `sticky.ts`'inin masaüstü karşılığı olurdu.
4. **Eklentinin release hattına eklenmesi.** Bugün elle `dist` yükleniyor. `release.yml`'e bir zip adımı ve Chrome Web Store yayını, masaüstü sürümüyle eklenti sürümünün ayrışmasını da engellerdi.
5. **`commands.rs` bölünmeli.** Dosya bugün 1000 satırın üzerinde ve dört ayrı konuyu taşıyor. Köprü bilerek kendi modülüne kondu ve `commands.rs`'e dokunmuyor, ama mevcut dosya kendi başına bölünmeyi hak ediyor.
6. **Köprü mesaj boyutu ve bellek.** PNG tek bir JSON çerçevesinde base64 taşınıyor; bu tek bir doğrulama yolu bırakıyor (her çerçeve JSON, ikili çerçeve asla kabul edilmiyor) ama en kötü durumda 64 MiB metin + 48 MiB bayt + çözülmüş piksel demek. İkili çerçeve + başlık mesajı bunu yarıya indirirdi, karşılığında bir durum makinesi ve bir saldırı yüzeyi eklerdi. Ölçüldükten sonra tekrar bakılmalı.
7. **`packages/protocol` bir `dist` üretmiyor.** Bugün kaynaktan import ediliyor. Vite dışında bir tüketici çıkarsa build adımı gerekir.
8. **Eklentinin i18n'i yok.** Masaüstü arayüzü İngilizce, eklenti de öyle. Depo genelinde bir çeviri kararı verilirse ikisi birlikte ele alınmalı.

---

## Uygulama sırasında alınan ek kararlar

Plan yazıldıktan sonra, Task 10 ve 11 uygulanırken ortaya çıkan ve planı bağlayan kararlar.

### `crates/frame`, tiplerin kendi crate'i (yeni)

Plan `crates/stitch`'in `snapdeck-capture`'a bağlanmasını, `Frame` ve `PixelFormat` tiplerini oradan
almasını söylüyordu. Uygulamada bunun bir bedeli çıktı: `snapdeck-capture` macOS'ta
`screencapturekit`'e bağlı, o da bir Swift köprüsü taşıyor, ve `snapdeck-stitch`'in test ikilisi
Swift çalışma zamanının rpath'ini almadığı için `cargo test` şu hatayla düşüyordu:

```
dyld: Library not loaded: @rpath/libswift_Concurrency.dylib
```

Bir rpath bayrağı bunu susturur, ama asıl sorun mimari: yalnız piksel bilen bir crate'in bir
platformun yakalama çerçevesine bağlanması, planın kendi "ekran, pencere ve dosya sistemi bilmez"
kuralının ihlaliydi. Bu yüzden tipler kendi crate'ine taşındı:

- `crates/frame` (`snapdeck-frame`): `types.rs` ve `error.rs`, saf veri, yalnız `serde` ve
  `thiserror` bağımlılığı.
- `crates/capture` aynı yüzeyi yeniden dışa açıyor (`pub use snapdeck_frame::{...}`), yani
  `apps/desktop` tarafında tek satır değişmedi.
- `crates/stitch` artık `snapdeck-frame`'e bağlı, `snapdeck-capture`'a değil.

Sonuç: `cargo test -p snapdeck-stitch` hiçbir ortam değişkeni olmadan geçiyor ve stitch bir daha
Swift'e bağlanmıyor. Task 12 `stitch`'i çağırırken bunu bilmeli: tipler `snapdeck_frame`'den gelir.

### Task 11, `TooLarge` kontrolünün yeri

Planın düzyazısı "önce şekil/ölçek uyumu, sonra `TooLarge`" diyordu; N8 testi ise bunun tersini
sabitliyordu. Sözleşme test tablosu olduğu için `TooLarge` öne alındı ve kareler henüz aynı boyutta
olduğu bilinmediğinden tutucu bir üst sınırla ölçülüyor (en geniş genişlik × bütün satırların
toplamı, doygun aritmetik).

Buna ek olarak: kontrol **tek kare kısayolunun da önüne** alındı. Plan tek kareyi hizalamadan
döndürüyordu ve bu doğru, ama sınırı da atlıyordu; tek bir 20.000 x 20.000 kare sınırı altı kat
aşmasına rağmen kabul edilip 1,6 GB açıyordu. Tek kare hâlâ hizalanmıyor, ama artık ölçülüyor.
Testi: `a_lone_frame_too_large_is_refused_as_well`, mutasyon kanıtı boyut bloğunu kısayolun arkasına
geri almak.

### Task 7, son adımın `sourceTop`'u ve planın hatası

Plan son adım için `sourceTop = viewportHeight - height` diyordu. Uygulamada bunun kırpılmış
sayfalarda yanlış olduğu bulundu: `truncated` durumunda son adımın `scrollY`'si
`compositeHeight - viewportHeight` değil `(n-1) * viewportHeight`'tır, dolayısıyla o formül yanlış
bandı yapıştırır. Ölçülmüş örnek: 4000/1000 CSS px, DPR 2, piksel bütçesi 5M, planın formülü
`sourceTop` 750 verir, doğrusu 0.

Yürürlükteki kural: `sourceTop = destTop - scrollY`. Kırpılmamış her girdide iki formül
matematiksel olarak aynıdır (son adımda `scrollY = documentHeight - viewportHeight` ve
`destTop = compositeHeight - height`, farkları `viewportHeight - height`), yalnız kırpma
durumunda ayrışırlar. Test tablosu değişmedi.

Ayrıca `measurePage` viewport yüksekliğini `documentElement.clientHeight`'tan okur (kutu yoksa
`innerHeight`'a düşer): tarayıcının kabul ettiği en büyük kaydırma `scrollHeight - clientHeight`'tır
ve `innerHeight` yatay kaydırma çubuğu kadar büyüktür, o farkla kurulan plan sessizce kırpılan bir
kaydırma ister.

### Güvenlik denetiminden gelen karar: köprü kendini de kanıtlar

Task 2 ve 3 bittikten sonra köprünün tamamı (Rust sunucu, zod şeması, eklenti istemcisi, manifest
izinleri) bağımsız bir güvenlik denetiminden geçti. Denetim tasarımın beş iddiasının beşini de
doğruladı, ama **tasarımın kendisinde** bir eksik buldu: spec 7.2 yalnız istemciyi doğruluyor,
sunucuyu doğrulayan hiçbir şey yok.

**Senaryo (HIGH):** Kullanıcı yetkisiyle çalışan yerel bir süreç `127.0.0.1:51837`'yi Snapdeck'ten
önce tutar. Eklenti bağlanır ve ilk çerçevede eşleştirme token'ını verir. Sahte sunucu `ready`
çerçevesini uydurur, çünkü o çerçeve hiçbir sır taşımıyor ve şemadan geçer. Eklenti tam sayfa
PNG'yi, sayfa adresini ve başlığını gönderir. Sahte sunucu `accepted` döner, eklenti rozeti
temizler ve indirmeye düşmez. Kullanıcı başarılı bir yakalama görür, verinin nereye gittiğini
öğrenmez. Kaybedilen şey tam olarak korunmaya çalışılan şeydir: kullanıcının oturum açmış
sayfalarının görüntüsü.

**Karar: karşılıklı kanıt.** Protokol şu şekilde genişler (sürüm hâlâ 1, çünkü hiçbir şey
yayınlanmadı):

- `hello` bir `nonce` taşır: eklentinin ürettiği 16 bayt rastgelelik, hex.
- `ready` bir `proof` taşır: `HMAC-SHA256(key = token, message = nonce)`, hex. Anahtar token
  dizesinin UTF-8 baytlarıdır (hex çözme belirsizliği olmasın diye).
- Eklenti `proof`u kendi hesabıyla karşılaştırmadan `fullPage` **göndermez**. Uyuşmazsa oturum
  kapanır, sayfa gönderilmez, indirmeye düşülür ve kullanıcıya sıradan bir "uygulama kapalı"
  mesajı değil, portta başka bir şeyin olduğunu söyleyen ayrı bir uyarı gösterilir.

Token'ı bilmeyen bir sahte sunucu `proof` üretemez, dolayısıyla senaryo kapanır. Bu, Native
Messaging'e geçmeden alınabilecek en yüksek kazanç; Native Messaging bağlantıyı işletim sistemi
seviyesinde belirli bir ikiliye bağlardı ama host manifest dosyası gerektirir ve spec 7.2 tek
kurulum adımı için bilinçli olarak loopback'i seçti.

Denetimin diğer bulguları ve kararları:

- **MEDIUM-1, el sıkışma zaman aşımı okuma başına.** Upgrade'den sonra, kimlik doğrulamadan önce
  Ping çerçeveleri damlatan bir eş saati sonsuza kadar sıfırlayabilir ve dört slotu tutabilir.
  Toplam bir son tarih (`Instant`) ve kimlik doğrulanmadan önce gelen kontrol çerçevesi sayısına
  tavan konur. (Tungstenite'ın kendi `AttackCheck`'i upgrade **öncesini** zaten sınırlıyor.)
- **MEDIUM-2, kimlik doğrulamadan önce 64 MiB tamponlanıyor.** `hello` beklenirken soketin mesaj
  sınırı birkaç KiB olur, token doğrulandıktan sonra tam sınıra çıkılır. `hello`'nun bütün alanları
  şemada zaten kısa.
- **MEDIUM-3, `MAX_PNG_BYTES` ve `MAX_IMAGE_PIXELS` tanımlı ama uygulanmıyor.** Sınırlar beyan
  edildikleri yerde uygulanır: `png_base64` uzunluğu, `width * height`, `url` 2048, `title` 1024,
  `request_id` uzunluk ve karakter kümesi, `device_pixel_ratio` sonlu ve makul aralık. zod
  tarafına da `.max()` eklenir ki iki ayna aynı sözleşmeyi söylesin.
- **LOW-1, `Host` başlığı denetlenmiyor.** DNS rebinding'i bugün `Origin` kapısı tutuyor (tarayıcı
  `Origin`'i doldurur ve sayfa onu değiştiremez), ama tek kapı olmamalı: `Host` da doğrulanır.
- **LOW-2, sunucudan gelen dizeler şemada sınırsız.** XSS yok (iki tüketici de metin sink'i), ama
  iki yönlü olduğu söylenen bir sınırda asimetri kalmamalı: `.max()` eklenir.
- **LOW-3, indirme dosya adı query string taşıyordu.** Düzeltildi (commit `a229132`): sıfırlama
  token'ları ve imzalı parametreler dosya adına, indirme geçmişine ve o klasörü senkronlayan
  servise gidiyordu.
- **Kabul edilen riskler:** loopback'te `ws://` (TLS burada sertifika sorunu getirir, süreç
  belleğini okuyabilen saldırgana karşı bir şey kazandırmaz); eklenti kimliğinin sabitlenmemesi
  (Web Store yayını gelirse sabitlenir, geliştirme yüklemesi için kaçış bırakılır); token'ın düz
  metin durması, **yalnız karşılıklı kanıt eklendikten sonra** kabul edilebilir.
- CI'a `cargo audit` eklenmesi Task 13'e yazıldı: denetim elle `Cargo.lock` okuyarak yapıldı,
  `tungstenite 0.30.0` bilinen advisory'lerden etkilenmiyor (RUSTSEC-2023-0065 0.20.1'de kapandı).
