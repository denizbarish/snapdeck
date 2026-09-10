# Snapdeck Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Snapdeck'i indirilip kurulabilir bir ürüne dönüştürmek: ayarlanabilir kısayollar ve kayıt yeri, etiketten tetiklenen bir release hattı, otomatik güncelleme, ve açık kaynak bir depodan beklenen katkı belgeleri. Sonunda gerçek bir `v0.1.0` yayınlanır.

**Architecture:** Ayarlar `tauri-plugin-store` ile tek bir JSON dosyasında durur ve tek bir Rust tipiyle okunur; hiçbir varsayılan iki yerde yazılmaz. Release, `tauri-action` ile etiketten tetiklenir ve updater manifestosunu da aynı koşuda üretir, böylece yayınlanan DMG ile manifesto asla ayrışamaz. İmzalama, sır varsa açılan, yoksa sessizce atlanan bir adımdır.

**Tech Stack:** Tauri 2.11.5, `tauri-plugin-store` 2.4.4, `tauri-plugin-updater`, `tauri-plugin-autostart`, `tauri-action`, GitHub Actions, React 19.

## Global Constraints

- Hedef platform macOS 14.0+. Kod, yorum, commit mesajı ve arayüz metinleri İngilizce; yalnızca `docs/superpowers/` Türkçe.
- Ağ çağrısı yalnızca güncelleme kontrolünde ve yalnızca kullanıcı açıkça istediğinde ya da açıkça izin verdiğinde yapılır. Telemetri yok, kullanım ölçümü yok.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm lint`, `pnpm test`, `pnpm build` her commit'te temiz.
- Mevcut davranış korunur: yakalama dosyayı yazar ve panoya kopyalar, editör üstüne açılır, gizleme garantisi bozulmaz.
- **Apple Developer ID yok.** İmzalama ve notarization, sır mevcutsa çalışan bir adımdır; yoksa build imzasız üretilir ve bu hem README'de hem sürüm notlarında açıkça yazılır. Sırların olmaması hattı kırmaz.
- Updater'ın kendi imza anahtarı Apple imzasından ayrıdır ve zorunludur: imzasız bir manifesto kabul edilmez.

## Kapsam dışı

- Windows ve Linux dağıtımı.
- Ekran kaydı (kullanıcı kararıyla v2).
- Full-page yakalama (kendi planı var, bu plandan sonra).
- Bulut, hesap, paylaşım linki.

---

### Task 1: Ayarlar

**Files:** `apps/desktop/src-tauri/src/settings.rs`, `settings.test` (inline), `apps/desktop/settings.html`, `apps/desktop/src/settings/`, `tray.rs`, `shortcuts.rs`, `lib.rs`, `commands.rs`

**Interfaces (birebir):**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub save_directory: Option<PathBuf>,   // None = ~/Pictures
    pub filename_template: String,
    pub default_format: SaveFormat,        // Png | Jpeg
    pub shortcuts: Shortcuts,
    pub launch_at_login: bool,
    pub open_editor_after_capture: bool,
}

impl Default for Settings { /* tek gerçek kaynak */ }
pub fn load(app: &AppHandle) -> Settings;              // bozuk dosyada varsayılana düşer ve loglar
pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String>;
```

**Kurallar:**
- Varsayılanlar **yalnızca** `Default for Settings` içinde yazılır. Bugün `DEFAULT_FILENAME_TEMPLATE` ve `Shortcuts::default` gibi dağınık sabitler varsa buraya taşınır; iki yerde yazılan bir varsayılan bulunursa görev başarısızdır.
- Kısayol değiştirildiğinde çakışma tespiti yapılır: kaydedilemeyen bir kısayol kullanıcıya söylenir ve **eski bağ geri yüklenir**, sessizce ölü tuş bırakılmaz. Bugün `register_shortcuts` yarım kayıtlı bir küme bırakabiliyor; bu görev onu da düzeltir.
- Kısayol yakalayıcı, kullanıcının bastığı kombinasyonu okur; serbest metin girişi yoktur.
- Kayıt klasörü seçicisi `tauri-plugin-dialog` ile açılır ve seçilen klasöre yazılabildiği **denenerek** doğrulanır.
- `launch_at_login` `tauri-plugin-autostart` ile gerçekten uygulanır, yalnızca saklanmaz.
- Tepsiye `Settings…` maddesi geri gelir ve bu pencereyi açar. Pencere tekildir: ikinci çağrı mevcut pencereyi öne getirir.
- Yakalama yolu ayarları okur: kayıt klasörü, ad şablonu ve varsayılan format artık sabit değildir.

- [ ] Testler: varsayılanların tek kaynaktan geldiği; bozuk JSON'un varsayılana düşüp loglaması; çakışan kısayolun reddedilip eskisinin geri gelmesi; yazılamayan klasörün reddi; şablonun kaydedilip yakalamada kullanılması.
- [ ] Ayarların değişmesi çalışan uygulamada anında etkili olur (yeniden başlatma gerekmez); kısayollar yeniden kaydedilir.
- [ ] Commit `feat(app): add a settings window`

---

### Task 2: Release hattı

**Files:** `.github/workflows/release.yml`, `README.md`, `docs/RELEASING.md`

**Kurallar:**
- `v*` etiketiyle tetiklenir, `macos-latest` üzerinde `tauri-action` ile derler, DMG ve `.app.tar.gz` üretir.
- Sürüm numarası tek kaynaktan gelir: etiket ile `tauri.conf.json` ve `Cargo.toml` uyuşmuyorsa iş **başarısız olur**, sessizce yanlış sürüm yayınlamaz.
- Release taslak olarak açılır, notlar şablondan doldurulur ve Gatekeeper uyarısı her sürüm notunda tekrarlanır.
- SHA-256 sağlama toplamları yayınlanır.
- İmzalama adımı `APPLE_CERTIFICATE` benzeri sırlar mevcutsa çalışır, yoksa atlanır ve bu durum sürüm notuna yazılır. Sır yokluğu hattı kırmaz.
- `docs/RELEASING.md`: sürüm çıkarma adımları, hangi sırların ne yaptığı, imzasız sürümün kullanıcı deneyimi.

- [ ] Hat, gerçek bir etiket atılmadan doğrulanır: `workflow_dispatch` ile veya `act` ile kuru koşu, ya da bir ön-sürüm etiketiyle (`v0.0.1-test`) yayınlanıp silinerek.
- [ ] README'ye Install bölümü: DMG indir, aç, Gatekeeper adımı, ilk çalıştırmada Ekran Kaydı izni.
- [ ] Commit `ci: build and publish a release from a tag`

---

### Task 3: Otomatik güncelleme

**Files:** `apps/desktop/src-tauri/src/updater.rs`, `lib.rs`, `tray.rs`, `.github/workflows/release.yml`, `docs/RELEASING.md`

**Kurallar:**
- `tauri-plugin-updater`, GitHub Releases'teki `latest.json` üzerinden.
- Updater anahtar çifti üretilir; **özel anahtar yalnızca GitHub sırlarında** durur, depoya asla girmez. Genel anahtar `tauri.conf.json` içindedir.
- Manifesto release iş akışında üretilir, elle yazılmaz.
- Kontrol kullanıcıya görünür: tepside `Check for Updates…`. Otomatik kontrol varsayılan olarak **kapalıdır** ve ayarlardan açılır; bu bir ağ çağrısıdır ve kullanıcının kararıdır.
- Güncelleme bulunduğunda sürüm notu gösterilir, kullanıcı onaylamadan indirme başlamaz.
- İmzasız bir manifesto veya doğrulanamayan imza reddedilir ve kullanıcıya söylenir.

- [ ] Testler: sürüm karşılaştırma (eşit, daha yeni, daha eski, ön-sürüm); imzası bozuk manifestonun reddi.
- [ ] Yerel bir statik manifesto ile uçtan uca denenir: eski sürüm, güncellemeyi görür, onay ister, indirir ve yeni sürüm açılır.
- [ ] Commit `feat(app): check for updates from GitHub Releases`

---

### Task 4: Açık kaynak belgeleri

**Files:** `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `.github/ISSUE_TEMPLATE/`, `.github/PULL_REQUEST_TEMPLATE.md`, `SECURITY.md`, `README.md`

**Kurallar:**
- `CONTRIBUTING.md`: kurulum, `pnpm install`'ın Chromium indirdiği, testlerin nasıl koşturulduğu, mimarinin bir paragrafı (yakalama çekirdeği, editör paketi, masaüstü kabuğu), commit ve PR beklentileri.
- Issue şablonları: hata (sürüm, macOS sürümü, adımlar, beklenen/gerçekleşen), özellik isteği.
- `SECURITY.md`: gizlilik açıkları için nasıl bildirim yapılacağı. Bu uygulama ekran içeriği okuyor ve redaksiyon vaat ediyor; bu bölüm ciddiye alınır.
- README: mimari şeması, ekran görüntüsü, bilinen sınırlamaların güncel hali.

- [ ] Commit `docs: add contributor and security documentation`

---

### Task 5: v0.1.0

- [ ] Sürüm numaraları hizalanır, `CHANGELOG.md` yazılır.
- [ ] Etiket atılır, hat koşar, DMG yayınlanır.
- [ ] **Temiz bir yoldan doğrulama:** DMG indirilir, `~/Applications` dışında bir yere açılır, Gatekeeper adımı README'nin yazdığı gibi işler, uygulama açılır, izin istenir, bir yakalama yapılır, editör açılır, kaydedilir. Bu, geliştirme ağacından değil, kullanıcının indirdiği şeyden doğrulanır.
- [ ] Sürüm notları: ne var, ne yok (ekran kaydı ve full-page henüz yok), bilinen sınırlamalar, imzasızlık uyarısı.
- [ ] Commit `chore: release v0.1.0`
