# RNS v3 - Spesifikasi Desain dan Test Matrix

Status: draft untuk diskusi. Belum ada perubahan kode.

Referensi:
- Kontrak v2: `C:\retium-sdk\examples\rns_v2\src\lib.rs`
- Frontend: `C:\Users\Hype GLK\Documents\rns-app`
- Dokumentasi runtime: `C:\retium-sdk\docs`

## 1. Tujuan


- Tidak ada RTM yang tertahan di kontrak setelah transaksi sukses.
- Data domain konsisten on-chain dan bisa dibaca dari browser/perangkat berbeda.
- API kontrak tetap sederhana untuk frontend statis tanpa backend wajib.
- Perubahan perilaku didokumentasikan dan diuji, termasuk jalur gagal.

## 2. Prinsip desain

1. Validasi seluruh input sebelum menulis state.
2. Setelah state ditulis, kegagalan pembayaran/refund harus memakai `fail()`, bukan `error()`, agar rollback berlaku juga untuk contract caller.
3. Setiap `send_rtm` harus diperiksa dengan `call::is_error_reply`. `send_rtm` mengembalikan `Ok(reply)` walaupun kontrak RTM membalas dengan error reply.
4. Harga disimpan dalam atomic unit. Read API mengembalikan amount sebagai string, bukan JSON number.
5. Aksi non-payable menolak `call.value() > 0`.
6. `init` diamankan dan treasury divalidasi.
7. Index harus tetap bisa dibaca dalam batas buffer dan fuel.
8. Tidak ada asumsi bahwa `error()` otomatis me-revert call dari kontrak lain.

## 3. Keputusan yang direkomendasikan

| No | Topik | Default yang disarankan | Alternatif |
|---|---|---|---|
| 1 | Pengamanan `init` | Hardcode `ADMIN` sebagai alamat multisig | First-caller + deploy/init segera + redeploy jika di-front-run |
| 2 | Treasury | Alamat multisig, bisa dirotasi oleh `ADMIN` lewat `set_treasury` | Immutable selamanya |
| 3 | Renew | Owner-only, aktif atau dalam grace period 30 hari | Siapa saja boleh renew |
| 4 | Grace period | 30 hari setelah expired; hanya owner lama boleh renew | Tanpa grace, langsung tersedia untuk umum |
| 5 | Sisa waktu minimum untuk jual/beli | 30 hari | Auto-renew saat pembelian atau harga pro-rata |
| 6 | Reserved names | Daftar hardcode + view `is_reserved` | Daftar di storage yang bisa diubah admin |
| 7 | Aturan hyphen | Tolak leading/trailing `-`, semua `-`, `--` di posisi 3-4, prefix `xn--` | Hanya leading/trailing |
| 8 | Recipient transfer | Hanya alamat `rete...` valid (72 karakter) | Izinkan contract id 64 hex juga |
| 9 | Panjang nama | 10 karakter (samakan dengan frontend) | 32 karakter (ubah frontend) |
| 10 | Index listing | Linked list aktif + pagination | Sharding prefix atau indexer off-chain |
| 11 | Index owner | Prefix index + pagination | Linked list per owner |
| 12 | Rescue dana tak terduga | Opsional, hanya `ADMIN`, dengan event | Tidak ada rescue |
| 13 | Format amount di read API | String | Number (tidak disarankan) |

Keputusan di atas belum final. Bagian 15 mencantumkan pertanyaan yang perlu dikonfirmasi.

## 4. Antarmuka kontrak

### 4.1 Write actions

| Method | Payable | Akses | Keterangan |
|---|---|---|---|
| `init(treasury)` | Tidak | `ADMIN` | Sekali saja; validasi format treasury; set treasury; emit event |
| `register(name, ext, months)` | Ya | Siapa saja | Validasi nama; izinkan re-register setelah grace; fee ke treasury; refund kelebihan |
| `renew(domain, months)` | Ya | Owner | Aktif atau dalam grace; perpanjang dari `max(now, expires_at)`; fee ke treasury; refund kelebihan |
| `update_records(domain, target, avatar)` | Tidak | Owner | Domain aktif; batasi panjang target/avatar |
| `list_for_sale(domain, price_atomic)` | Tidak | Owner | Domain aktif; sisa waktu minimal; `price_atomic == 0` menghapus listing |
| `buy_domain(domain)` | Ya | Bukan owner | Listing aktif; sisa waktu minimal; bayar seller penuh; refund kelebihan |
| `transfer_domain(domain, recipient)` | Tidak | Owner | Domain aktif; recipient valid; hapus listing |
| `set_treasury(new_treasury)` | Tidak | `ADMIN` | Opsional; validasi format; emit event |
| `rescue(to)` | Tidak | `ADMIN` | Opsional; hanya untuk saldo tak terduga; emit event |

### 4.2 Read actions

| Method | Keterangan |
|---|---|
| `get_info(domain)` | JSON: owner, target, avatar, sale_price (string), expires_at (string), is_expired, is_listed |
| `get_domains_by_owner(owner, after, limit)` | JSON: daftar domain + `next` untuk pagination |
| `get_listed_domains(after, limit)` | JSON: daftar domain yang listed + `next` |
| `quote(name, months)` | Harga atomic unit untuk nama dan durasi; dipakai frontend agar tidak duplikasi rumus |
| `is_reserved(name)` | Boolean |
| `get_config()` | treasury, admin, min_remaining_ms, grace_ms, max_name_length, version |

`after` dan `limit` boleh opsional. Jika tidak didukung, sediakan method lama tanpa pagination dan method baru dengan pagination.

## 5. Model storage

Semua key menggunakan string. Domain tidak boleh mengandung `:` atau `|`, dan alamat yang disimpan harus sudah divalidasi.

| Key | Value |
|---|---|
| `treasury` | alamat treasury |
| `admin` | alamat admin (jika tidak hardcode) |
| `owner:<domain>` | alamat owner |
| `target:<domain>` | target resolusi |
| `avatar:<domain>` | avatar |
| `expires:<domain>` | timestamp ms |
| `sale_price:<domain>` | harga atomic |
| `listing:<domain>` | `next|prev` untuk linked list |
| `listing_head`, `listing_tail` | domain pertama/terakhir |
| `owned:<owner>:<domain>` | `1` atau kosong |
| `owned_head:<owner>` | opsional, jika owner index memakai linked list |

Catatan: penulisan nilai kosong menghapus isi key, tetapi nama key tetap ada di daftar key chain. Ini alasan index global perlu strategi yang tidak memindai seluruh histori.

## 6. Aturan validasi

### 6.1 Nama

- Panjang 3 sampai `MAX_NAME_LENGTH`.
- Hanya `a-z`, `0-9`, dan `-`.
- Tidak diawali atau diakhiri `-`.
- Tidak semua karakter `-`.
- Tidak mengandung `--` pada posisi 3-4 (opsional, untuk menghindari kebingungan punycode).
- Tidak diawali `xn--`.
- Tidak ada di daftar reserved.
- Extension harus `.rtm`.

Daftar reserved awal yang disarankan: `admin`, `retium`, `rtm`, `rtd`, `www`, `api`, `mail`, `support`, `help`, `status`, `blog`, `docs`, `app`, `wallet`, `explorer`, `treasury`, `official`, `system`, `root`, `null`.

### 6.2 Durasi

Hanya `3`, `6`, `12`, `36` bulan.

### 6.3 Harga

- `price_atomic` adalah `u128` dalam atomic unit.
- `0` hanya untuk unlist, bukan untuk menjual gratis.
- `buy_domain` menolak listing dengan harga `0`.
- Pertimbangkan batas atas harga untuk menghindari input tidak wajar; batas ini bukan pengganti validasi pembayaran.

### 6.4 Alamat

Alamat Retium yang valid:
- Panjang tepat 72 karakter.
- Prefix `rete`.
- 68 karakter setelah prefix adalah lowercase hex.
- Checksum 4 hex terakhir tidak bisa diverifikasi on-chain karena host tidak mengekspos blake3. Validasi checksum tetap di client.
- `transfer_domain` dan `init` minimal memvalidasi prefix, panjang, dan lowercase hex.
- `transfer_domain` menolak recipient yang sama dengan caller.

Jika ingin mengizinkan kontrak sebagai owner, tambahkan format contract id 64 hex secara eksplisit. Default v3: hanya `rete...`.

### 6.5 Target dan avatar

- Target maksimal 256 byte.
- Avatar maksimal 512 byte.
- Jika target dimaksudkan sebagai alamat, validasi format `rete...`.
- Jika target berupa URL/IPFS, validasi skema yang diizinkan dan batasi panjang.

### 6.6 Value

- `init`, `update_records`, `list_for_sale`, `transfer_domain`, `set_treasury`, `rescue`: `call.value() == 0`.
- `register`, `renew`, `buy_domain`: `call.value() >= harga`; refund kelebihan.
- `buy_domain` membayar seller tepat sebesar `sale_price`, bukan seluruh `call.value()`.

## 7. Semantik pembayaran dan kegagalan

Urutan yang disarankan untuk aksi payable:

1. Baca dan validasi semua input.
2. Hitung harga dan fee.
3. Validasi treasury dan listing.
4. Tulis state (ownership, expiry, listing, index).
5. Kirim fee ke treasury dan/atau seller. Periksa `is_error_reply`.
6. Refund kelebihan. Periksa `is_error_reply`.
7. Emit event.
8. Jika langkah 5-7 gagal, panggil `fail()` agar seluruh call rollback, termasuk untuk contract caller.

Catatan:
- `error()` aman untuk user call karena chain me-revert seluruh call.
- `error()` tidak cukup untuk contract caller karena reply error bisa diabaikan.
- `fail()` selalu membatalkan writes/events/transfers dan memberi caller `Err`.
- Jangan memakai `match send_rtm { Ok(_) => {} }` tanpa memeriksa isi reply.
- Saldo kontrak harus 0 setelah setiap call sukses. Uji invariants ini.

## 8. Expiry, renew, dan listing

- `expires_at` adalah timestamp ms.
- `is_expired` untuk write actions harus menganggap `expires_at == 0` sebagai expired/invalid.
- Grace period 30 hari setelah expired:
  - Hanya owner lama yang boleh `renew`.
  - Pihak lain belum boleh `register` selama grace.
  - Listing lama tidak bisa dibeli; owner boleh unlist.
- Setelah grace:
  - Siapa saja boleh `register`.
  - `register` membersihkan owner index lama dan listing lama.
- `renew`:
  - Hanya owner.
  - Aktif atau dalam grace.
  - `new_expires = max(now, expires_at) + duration_ms`.
  - Fee dihitung dengan rumus yang sama seperti register.
  - Refund kelebihan.
  - Emit `domain_renewed`.
- `list_for_sale`:
  - Hanya owner.
  - Domain aktif.
  - Sisa waktu minimal 30 hari.
  - `price == 0` selalu boleh untuk unlist, bahkan jika domain sudah expired, agar listing tidak menggantung.
- `buy_domain`:
  - Listing harus ada dan harga > 0.
  - Domain belum expired.
  - Sisa waktu minimal 30 hari.
  - Buyer bukan owner.
  - Pembayaran minimal sebesar harga; refund kelebihan.
  - Seller menerima harga penuh.
- `transfer_domain`:
  - Hanya owner.
  - Domain aktif.
  - Recipient valid dan bukan caller.
  - Hapus listing.
  - Target direset ke recipient.
- Domain expired tetap muncul di `get_domains_by_owner` untuk histori, tetapi frontend harus menampilkan status expired dan aksi renew/re-register, bukan Sell/Transfer.

## 9. Init, admin, dan treasury

Default:
- `ADMIN` adalah alamat multisig yang di-hardcode.
- `init` hanya bisa dipanggil `ADMIN`.
- `init` memvalidasi format treasury.
- `init` menolak `call.value() > 0`.
- `init` hanya bisa sekali.
- `set_treasury` hanya `ADMIN`, memvalidasi format, emit `treasury_updated`.
- `rescue` opsional, hanya `ADMIN`, emit `rescue`.

Alternatif jika kode ingin reusable:
- `init` mencatat first caller sebagai admin.
- Deploy dan init harus dilakukan segera dalam satu alur otomatis.
- Verifikasi treasury setelah init sebelum mengumumkan atau mendanai kontrak.
- Jika di-front-run, redeploy dengan metadata `name` berbeda sehingga contract id berubah.

Dokumentasikan wewenang admin:
- Mengubah treasury.
- Rescue dana tak terduga (jika diaktifkan).
- Tidak boleh mengubah owner domain atau harga listing.

## 10. Event

| Event | Field |
|---|---|
| `initialized` | treasury, admin, by |
| `treasury_updated` | old, new, by |
| `domain_registered` | domain, owner, price, expires_at |
| `domain_renewed` | domain, owner, price, expires_at |
| `records_updated` | domain, target, avatar, by |
| `domain_listed` | domain, price, by |
| `domain_unlisted` | domain, by |
| `domain_sold` | domain, buyer, seller, price |
| `domain_transferred` | domain, from, to |
| `rescue` | to, amount, by |

Semua nilai string harus di-escape dengan `emit_fields` atau `json_string`.

## 11. Format read API

### get_info

```json
{
  "owner": "rete...",
  "target": "rete...",
  "avatar": "",
  "sale_price": "50000000000000000",
  "expires_at": "1801933786428",
  "is_expired": false,
  "is_listed": true
}
```

`sale_price` dan `expires_at` dikembalikan sebagai string agar tidak kehilangan presisi di JavaScript.

### get_domains_by_owner / get_listed_domains

```json
{
  "domains": ["alpha.rtm", "beta.rtm"],
  "next": "beta.rtm"
}
```

`next` bernilai `null` atau string kosong jika sudah habis. Frontend memakai tombol **Load More** dan berhenti saat `next` kosong.

### quote

```json
{ "price": "1250000000000000000", "months": "3" }
```

## 12. Perubahan frontend

- `config.js`: contract id baru, chain id, RPC.
- `get_info`: parse `sale_price` dan `expires_at` sebagai string.
- `quote`: gunakan hasil kontrak, jangan duplikasi rumus harga.
- Domain expired: tampilkan status expired/grace dan tombol Renew. Setelah grace berakhir, tampilkan bahwa nama tersedia untuk registrasi ulang.
- Listing expired: jangan tampilkan tombol Buy.
- Sisa waktu < 30 hari: tampilkan peringatan dan nonaktifkan Sell/Buy sampai renew.
- Validasi recipient: tepat 72 karakter `rete` + 68 lowercase hex.
- Validasi nama: samakan aturan reserved dan hyphen dengan kontrak.
- Pagination: dukung `{domains, next}` untuk My Domains dan Marketplace dengan tombol Load More.
- Renew: modal durasi 3/6/12/36 bulan; harga diambil dari `quote`.
- Error `fail()`: tampilkan pesan generik "Transaction failed" dan sarankan cek explorer.
- Cache: tetap fallback, tetapi data on-chain adalah sumber utama.
- Query multi-argumen: kirim koma literal pada parameter `args` (node tidak memecah `%2C`).

## 13. Migrasi

- Deploy v3 sebagai contract id baru.
- Gunakan `evolves_from` ke contract id v2 jika deployer sama dan versi lebih tinggi.
- `evolves_from` tidak memindahkan storage. Domain v2 tidak otomatis pindah.
- Opsi migrasi:
  1. Re-register manual untuk domain testnet.
  2. Snapshot event `domain_registered` dari v2, lalu sediakan proses migrasi dengan signature/merkle (lebih kompleks).
  3. Untuk mainnet, umumkan tanggal cut-off dan biarkan pemilik melakukan migrasi.
- Frontend menampilkan contract id baru dan status migrasi.

## 14. Test matrix

### 14.1 Unit test off-chain

| ID | Kasus | Ekspektasi |
|---|---|---|
| U01 | `init` oleh non-admin | Error |
| U02 | `init` dua kali | Error |
| U03 | `init` dengan treasury invalid | Error |
| U04 | `init` dengan value > 0 | Error |
| U05 | Nama < 3, > max, karakter invalid | Error |
| U06 | Nama leading/trailing hyphen, all-hyphen, `xn--` | Error |
| U07 | Nama reserved | Error |
| U08 | Extension selain `.rtm` | Error |
| U09 | Durasi selain 3/6/12/36 | Error |
| U10 | `update_records` oleh non-owner | Error |
| U11 | `update_records` pada domain expired | Error |
| U12 | `update_records` target/avatar terlalu panjang | Error |
| U13 | `list_for_sale` oleh non-owner | Error |
| U14 | `list_for_sale` pada domain expired | Error |
| U15 | `list_for_sale` sisa waktu < 30 hari | Error |
| U16 | `list_for_sale` price 0 pada domain expired | Sukses unlist |
| U17 | `buy_domain` pada listing tidak ada | Error |
| U18 | `buy_domain` oleh owner | Error |
| U19 | `buy_domain` pada domain expired | Error |
| U20 | `buy_domain` sisa waktu < 30 hari | Error |
| U21 | `buy_domain` dengan value < harga | Error |
| U22 | `transfer_domain` recipient invalid | Error |
| U23 | `transfer_domain` recipient sama dengan caller | Error |
| U24 | `transfer_domain` pada domain expired | Error |
| U25 | Aksi non-payable dengan value > 0 | Error |

### 14.2 Integration testnet

| ID | Kasus | Ekspektasi |
|---|---|---|
| I01 | Register normal | Fee ke treasury, refund 0, saldo kontrak 0 |
| I02 | Register dengan kelebihan bayar | Fee ke treasury, refund tepat, saldo kontrak 0 |
| I03 | Register saat treasury invalid | Transaksi gagal, state tidak berubah, saldo kontrak 0 |
| I04 | Re-register setelah expired | Owner lama dibersihkan dari index, listing lama hilang, expiry baru benar |
| I05 | Renew oleh owner | Expiry bertambah dari expiry lama, fee ke treasury, saldo kontrak 0 |
| I06 | Renew oleh non-owner | Gagal |
| I07 | Renew dalam grace period | Sukses |
| I08 | Register oleh pihak lain dalam grace period | Gagal |
| I09 | Register oleh pihak lain setelah grace | Sukses |
| I10 | List normal | `is_listed` benar, index listing bertambah |
| I11 | Unlist normal | `is_listed` false, index listing berkurang |
| I12 | Unlist setelah expired | Sukses, listing hilang |
| I13 | Buy normal | Seller menerima harga penuh, buyer menjadi owner, saldo kontrak 0 |
| I14 | Buy dengan kelebihan bayar | Seller menerima harga penuh, buyer mendapat refund, saldo kontrak 0 |
| I15 | Buy saat seller payout error reply | Transaksi gagal, state tidak berubah, saldo kontrak 0 |
| I16 | Buy saat refund error reply | Transaksi gagal, state tidak berubah, saldo kontrak 0 |
| I17 | Transfer normal | Owner/index berubah, listing hilang, saldo kontrak 0 |
| I18 | Transfer ke recipient invalid | Gagal, domain tetap milik pengirim |
| I19 | Transfer dengan value > 0 | Gagal |
| I20 | Panggil aksi non-payable dengan value | Gagal, value kembali |
| I21 | `set_treasury` oleh admin | Treasury berubah, event emitted |
| I22 | `set_treasury` oleh non-admin | Gagal |
| I23 | Rescue saldo tak terduga | Saldo kembali ke target, event emitted |

### 14.3 Failure injection

Karena `send_rtm` bisa mengembalikan `Ok(error reply)`, test harus memaksa kondisi tersebut:

- Treasury invalid atau kontrak treasury menolak RTM.
- Recipient payout invalid (jika validasi recipient sengaja dilewati untuk test).
- Mock RTM transfer yang mengembalikan error reply.

Setiap kasus harus membuktikan:
- State ownership/expiry/listing tidak berubah.
- Saldo kontrak tetap 0.
- Buyer/seller tidak kehilangan dana.
- Transaksi berstatus gagal.

### 14.4 Test index dan batas

| ID | Kasus | Ekspektasi |
|---|---|---|
| X01 | 100 listing aktif | `get_listed_domains` mengembalikan semua |
| X02 | 1.000+ listing historis | Query tetap berhasil atau pagination bekerja |
| X03 | Owner dengan banyak domain | `get_domains_by_owner` pagination bekerja |
| X04 | Listing dihapus lalu ditambah lagi | Tidak ada duplikat, linked list konsisten |
| X05 | Head/tail dihapus | Head/tail baru benar |

### 14.5 Frontend

| ID | Kasus | Ekspektasi |
|---|---|---|
| F01 | Domain expired | Tampil status expired dan aksi renew/re-register |
| F02 | Listing expired | Tombol Buy tidak muncul |
| F03 | Sisa waktu < 30 hari | Peringatan dan Sell/Buy nonaktif |
| F04 | Harga atomic besar | Tidak kehilangan presisi |
| F05 | Recipient invalid | Ditolak sebelum transaksi |
| F06 | Nama reserved/hyphen invalid | Ditolak sebelum transaksi |
| F07 | Wallet sama di browser berbeda | My Domains identik dari chain |
| F08 | RPC gagal | Cache fallback digunakan dengan peringatan |

## 15. Keputusan final

Semua default berikut disetujui dan diimplementasikan:

1. `ADMIN` di-hardcode ke alamat deployer `rete4203...641`. Pindah ke multisig berarti deploy baru.
2. Treasury dapat dirotasi oleh `ADMIN` lewat `set_treasury`.
3. `renew` hanya boleh dilakukan owner, selama domain aktif atau dalam grace period.
4. Grace period 30 hari setelah expired; hanya owner lama yang boleh renew.
5. Listing dan pembelian memerlukan sisa waktu minimal 30 hari.
6. Reserved names di-hardcode di kontrak dan dapat dibaca lewat `is_reserved`.
7. Hyphen: tolak leading/trailing dan `--` pada posisi 3-4 (penanda punycode).
8. `transfer_domain` hanya menerima alamat `rete` valid 72 karakter.
9. Panjang nama 3-10 karakter.
10. Index listing memakai linked list aktif + pagination.
11. Index owner memakai prefix `owned:<owner>:` + pagination.
12. `rescue` hanya untuk `ADMIN`; saldo nol mengembalikan "Nothing to rescue".

## 16. Status implementasi

### Kontrak

- Source: `C:\retium-sdk\examples\rns_v3\src\lib.rs`
- Contract id: `dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77`
- Code hash: `ddf2690c18a55dc38b6489a1cbd7ddc3588bb863d05f4856aff7d314687d8db9`
- Deploy tx: `tx_37f9b3dbeb6f35167aaf015d29518cf9`
- `init` tx: `tx_dc83d0d8f637ae4a0a9acf856ae8393e`
- Versi: `v3.0.0`; treasury dan admin: `rete4203660202c54aeea6d540c2063eb928a049201dbf50a50123b7ab7dd608c44da641`
- `retium verify` cocok dengan wasm release (81.388 byte). Salinan wasm ada di `rns-app/contracts/rns_v3.wasm`. Build ulang: `cd contract && cargo build --release --locked --target wasm32-unknown-unknown -p rns_v3` (tanpa flag tambahan) menghasilkan byte yang identik dengan kontrak on-chain.

### Test yang sudah lulus

- 27 unit test `cargo test -p rns_v3` (sebelumnya 18).
- Register dengan overpayment: fee ke treasury, refund tepat, saldo kontrak 0.
- List lalu buy: seller menerima harga penuh, pembeli menerima refund, saldo kontrak 0.
- Renew oleh owner baru: expiry bertambah sesuai durasi, fee ke treasury.
- Transfer: owner dan index pindah, listing dibersihkan.
- Non-payable call dengan value: gagal, value kembali, state tidak berubah.
- Reserved name, underpayment, init ganda, `set_treasury` non-admin, recipient invalid, renew non-owner: semuanya ditolak.
- Rotasi treasury ke akun lain dan kembali: fee mengikuti treasury baru.
- Pagination listing dan owner bekerja, termasuk `next`.
- Update harga listing tidak membuat duplikat; unlist membersihkan head/tail.
- Frontend: `node --check` lulus; quote, availability, owner index, marketplace, pagination Load More, status expired/grace/near-expiry, renew preview, dan validasi recipient diuji di browser terhadap data testnet.

Test host menambahkan kontrol waktu dan simulasi pembayaran, sehingga jalur berikut sekarang diuji di level kontrak:

- Renew masih diterima tepat pada milidetik terakhir grace period, dan ditolak satu milidetik setelahnya.
- Renew dalam grace menghitung expiry baru dari waktu sekarang, bukan dari expiry lama.
- Register oleh pihak lain ditolak selama grace dan diterima satu milidetik setelah grace berakhir; index owner lama dibersihkan.
- `list_for_sale` dan `buy_domain` menerima sisa waktu tepat 30 hari dan menolak 30 hari dikurangi satu milidetik.
- Buy membayar seller langsung dari kontrak sebesar harga listing dan mengembalikan kelebihan ke buyer; tidak ada dana yang tertinggal di kontrak.
- Error reply dari RTM saat payout seller/treasury memicu trap, sehingga chain membatalkan seluruh perubahan state.
- Renew dengan kelebihan bayar mengembalikan selisih ke owner.

Host test memakai `retium_contract::test_host`, yang hanya aktif pada build non-wasm. Build wasm release dengan dan tanpa perubahan test host menghasilkan byte yang sama (81.388 byte); kontrak yang sudah dideploy tidak berubah dan tidak perlu redeploy.

### Batasan pengujian langsung

- Peralihan waktu nyata melewati grace period tidak ditunggu di testnet; batas waktu sudah diuji deterministik lewat host test.
- Listing/pembelian pada domain yang tersisa kurang dari 30 hari tidak ditunggu secara waktu nyata; batas 30 hari sudah diuji lewat host test.
- Failure injection untuk error reply RTM tidak bisa dipaksakan ke node testnet; semantik `is_error_reply` + `fail()` diuji lewat host test.
- Migrasi domain v2 ke v3; testnet memakai re-register manual. Kontrak v2 lama masih menyimpan dana yang tidak dapat ditarik karena tidak memiliki fungsi withdraw.

## 17. Non-goals v3

- Tidak ada backend/indexer wajib.
- Tidak ada lelang, offer, atau escrow.
- Tidak ada subdomain.
- Tidak ada pembayaran fiat.
- Tidak ada multi-chain.
- Tidak ada upgrade kontrak in-place; versi baru adalah deploy baru.
