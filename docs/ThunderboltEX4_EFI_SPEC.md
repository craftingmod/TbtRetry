# ThunderboltEX 4 — UEFI 부팅 성공 판별 명세

> **대상**: ASUS ROG STRIX Z690-A GAMING WIFI D4, BIOS 3101, ThunderboltEX 4  
> **작성 기준**: 2026-11-02까지의 Good/Bad PCIe·ACPI 캡처 및 BIOS 3101 정적 역공학  
> **목적**: Windows Boot Manager를 실행하기 **전**에 이번 부팅의 Thunderbolt 초기화가 정상적으로 완료되었을 가능성을 판정한다. 선택적으로 실패 시 재부팅하는 EFI 애플리케이션의 구현 기준으로 사용한다.  
> **분석·판별 동작**: 원칙적으로 **읽기 전용**. Thunderbolt mailbox 실행, PCIe 제어 레지스터 변경, BIOS/NVRAM 설정 변경, BIOS 패치는 필요하지 않다.

## 1. 요약 — 무엇을 검사해야 하나?

**단일 신호만으로 성공을 판정하지 말 것.** 이전에 확보한 두 종류의 독립적인 증거를 함께 확인한다.

1. **PCIe 링크(실시간 하드웨어 상태)**: Root Port `0000:00:1c.4`의 PCI Express **Link Status**에서 속도 `8 GT/s`, 폭 `x4`, `Training=0`, `DLLLA=1`을 확인한다.
2. **ACPI 테이블(이번 부팅의 펌웨어 초기화 결과)**: `SSDT`의 **OEM Table ID**를 열거하여 `DTbtSsdt`와 `AdlS_Tbt`가 모두 있고, `AdlS_Rvp`는 없는지 확인한다.

**엄격한 `PASS`** = 두 범주가 모두 Good 기준을 만족할 때. `PASS`이면 EFI 애플리케이션은 Windows Boot Manager로 체인로드할 수 있다. 그 외에는 `FAIL / DEGRADED / UNKNOWN`을 구분해야 한다.

**중요**: 이는 *Windows 진입 시점의 Thunderbolt 정상 동작에 대한 강한 간접 판정*이지, 실제 외부 Thunderbolt 장치의 연결·인증·USB 인식까지 100% 보증하는 검사가 아니다.

## 2. 원본 Good/Bad 자료로 확정한 기준값

### 2.1 Root Port 식별

| 항목 | 관측값 | 용도 |
|---|---|---|
| PCI Segment:Bus:Device.Function | `0000:00:1c.4` | 분석 대상 Root Port |
| Vendor ID | `0x8086` (Intel) | 다른 장치 오검출 방지 |
| Device ID | `0x7ABC` | 이 보드의 캡처와 동일한 포트인지 확인 |
| PCI Express Capability | `0x40` | **캡처 시 관측값**; 구현에서는 capability list를 탐색 |
| PCI Express Link Control | `Cap + 0x10 = 0x50` | 진단 출력에만 사용, 쓰기 금지 |
| PCI Express Link Status | `Cap + 0x12 = 0x52` | **주요 판별 레지스터(16비트)** |
| Link Control 2 / Target Link Speed | `Cap + 0x30 = 0x70` | 진단 보조 정보. 현재 협상 속도와 혼동하지 말 것 |

PCIe Capability `0x40`과 BDF는 **이 보드의 두 스냅샷에서 확인된 값**이다. 범용 구현에서 `0x52`를 고정해서 읽지 말고 Capability ID `0x10`을 찾아 `Cap + 0x12`를 읽는다. BDF 역시 BIOS 변경이나 다른 포트 구성에서는 재확인이 필요하다.

### 2.2 실제 측정 결과

| 항목 | **Good** | **Bad** |
|---|---:|---:|
| Root Port Vendor:Device | `8086:7ABC` | `8086:7ABC` |
| PCI Express Cap offset | `0x40` | `0x40` |
| Link Control (`0x50`) | `0x0C40` | `0x0C42` |
| **Link Status (`0x52`)** | **`0x3043`** | **`0x1801`** |
| 현재 링크 속도 (`[3:0]`) | `0x3` = 8 GT/s | `0x1` = 2.5 GT/s |
| 현재 링크 폭 (`[9:4]`) | `0x4` = x4 | `0x0` = x0 |
| Link Training (`bit 11`) | `0` | `1` |
| Data Link Layer Link Active (`bit 13`) | `1` | `0` |
| Target Link Speed (`0x70`, `[3:0]`) | `0x3` | `0x1` |
| `DTbtSsdt` (OEM Table ID) | 있음: 10,175 B | 없음 |
| `AdlS_Tbt` (OEM Table ID) | 있음: 8,952 B | 없음 |
| `AdlS_Rvp` (OEM Table ID) | 없음 | 있음: **36 B (헤더만)** |
| `TbtTypeC` (OEM Table ID) | 있음: 9,047 B | 있음: 9,047 B (동일 바이트) |

**판별에 쓰면 안 되는 것:** `TbtTypeC` 존재 여부. Good/Bad에 모두 나타났으며 **양쪽 바이트가 완전히 동일**하다. `SSDT1`이나 `SSDT6` 같은 **순번도 사용하면 안 된다**. Good/Bad에서 SSDT 개수가 달라져 번호가 밀린다.

### 2.3 Link Status 디코딩

```c
// pcie_cap = PCI Configuration Space에서 찾은 PCI Express Capability(ID 0x10)의 offset
uint16_t link_status = PciRead16(pcie_cap + 0x12);
uint8_t speed    =  link_status        & 0x0F;  // 1: 2.5, 2: 5, 3: 8 GT/s
uint8_t width    = (link_status >> 4) & 0x3F;  // negotiated link width
bool training    = (link_status & (1u << 11)) != 0;
bool dll_active  = (link_status & (1u << 13)) != 0;

bool good_link = (speed == 3) && (width == 4) && !training && dll_active;
```

다른 PCIe 포트의 Link Status를 읽지 않았는지 **Vendor/Device ID + BDF + Capability ID**를 먼저 검증한다. `0xFFFF` 등 읽기 오류나 PCI I/O 실패는 **링크 실패로 추정하여 무한 재부팅하기보다 `UNKNOWN`으로 처리**한다.

## 3. ACPI SSDT 판별 방법

### 3.1 필드와 위치

ACPI SDT Header(최소 **36바이트**)에서 필요한 필드:

| 바이트 오프셋 | 길이 | 필드 | 설명 |
|---:|---:|---|---|
| `0x00` | 4 | `Signature` | `SSDT`인지 확인 |
| `0x04` | 4 | `Length` | 테이블 전체 길이, 리틀엔디언 |
| `0x09` | 1 | `Checksum` | 전체 테이블 8비트 합이 0인지 검사 |
| `0x0A` | 6 | `OemId` | `INTEL ` 또는 `ALASKA` 등 |
| **`0x10`** | **8** | **`OemTableId`** | `DTbtSsdt` / `AdlS_Tbt` / `AdlS_Rvp` / `TbtTypeC` |

`Signature`는 네 항목 모두 `SSDT`다. **`OemTableId`가 차이를 만든다.** 8바이트 고정 길이 문자열이므로 NUL 종단 문자열 취급하지 말고 `memcmp(..., 8)`로 비교한다.

- `DTbtSsdt`: `Signature=SSDT`, `OemId="INTEL "`, `OemTableId="DTbtSsdt"`, `Length=10175` (Good).
- `AdlS_Tbt`: `Signature=SSDT`, `OemId="ALASKA"`, `OemTableId="AdlS_Tbt"`, `Length=8952` (Good).
- `AdlS_Rvp`: `Signature=SSDT`, `OemId="ALASKA"`, `OemTableId="AdlS_Rvp"`, `Length=36` (Bad).
- `TbtTypeC`: `Signature=SSDT`, `OemId="ALASKA"`, `OemTableId="TbtTypeC"`, `Length=9047` (양쪽).

길이는 이번 BIOS 3101 캡처에 대한 **일관성 검사와 진단 정보**로 사용한다. 향후 다른 BIOS가 테이블 길이를 변경했다고 해서 정상 테이블을 자동으로 `FAIL` 처리하지는 말고 `UNKNOWN / unsupported baseline`으로 분류할 것. Checksum 불량 또는 Header 길이 오류도 `UNKNOWN`이다.

### 3.2 UEFI에서 열거하는 방법

**우선 경로: `EFI_ACPI_SDT_PROTOCOL`.**

1. `gBS->LocateProtocol(&gEfiAcpiSdtProtocolGuid, ...)`로 프로토콜 획득.
2. `GetAcpiTable(Index, &Table, &Version, &TableKey)`를 `Index = 0, 1, ...` 순으로 호출한다.
3. `EFI_NOT_FOUND`가 나올 때까지 열거한다(기타 오류는 `UNKNOWN`).
4. 각 Table의 Header 유효성을 확인한 뒤 `Signature == "SSDT"`인 테이블의 `OemTableId[8]`을 비교한다.
5. `found_dtbt`, `found_adls_tbt`, `found_adls_rvp`, `found_tbttypec`를 기록한다. 동일 ID 중복, 서로 모순된 조합은 `UNKNOWN`으로 처리한다.

**대체 경로**: `EFI_ACPI_SDT_PROTOCOL`이 제공되지 않으면 EFI System Table의 `ConfigurationTable`에서 **ACPI 2.0 RSDP**를 찾고, RSDP → XSDT → 각 SDT Header를 **읽기 전용**으로 열거한다. 포인터·길이·checksum 검증이 필요하다. 단순히 UEFI Shell에서 ACPI가 보인다고 `GetAcpiTable()` 프로토콜의 존재까지 보장되는 것은 아니다.

EFI 프로그램에서 HOB를 직접 확인할 수 있다고 가정하지 말 것. **ACPI 테이블은 HOB 존재 여부가 아니라 DXE 초기화 결과를 간접 검증하는 신호**다.

## 4. 실제 성공/실패 판별 정책

구현 시 `PASS`, `FAIL`, `DEGRADED`, `UNKNOWN`의 4상태로 구분한다. 엄격한 무인 자동 부팅 정책의 기본값은 다음과 같다.

| ACPI 상태 | PCIe 상태 | 판정 | 기본 처리 |
|---|---|---|---|
| `DTbtSsdt` + `AdlS_Tbt` 존재, `AdlS_Rvp` 없음 | Gen3 x4, `Training=0`, `DLLLA=1` | **PASS** | Windows 체인로드 |
| `AdlS_Rvp` 존재, 두 Good 테이블 없음 | x0 / Training=1 / DLLLA=0 | **FAIL** | 제한된 자동 재시도 가능 |
| `AdlS_Rvp` 존재, 두 Good 테이블 없음 | 링크가 우연히 활성 | **FAIL** | DXE 테이블 경로 불일치; 제한된 재시도 가능 |
| 두 Good 테이블 존재, `AdlS_Rvp` 없음 | x0 / Training=1 / DLLLA=0 | **FAIL** | 물리 링크 비정상; 제한된 재시도 가능 |
| 두 Good 테이블 존재, `AdlS_Rvp` 없음 | x4, DLLLA=1, Training=0이나 Gen1/2 속도 | **DEGRADED** | 로그·사용자 선택(기본 자동 재부팅 없음) |
| 한쪽 Good 테이블만 존재 / Good·Bad 테이블 공존 | 어떤 상태든 | **UNKNOWN** | 로그·사용자 선택; 무한 재부팅 금지 |
| ACPI 조회 실패, 잘못된 Header/Checksum, PCI 컨트롤러 불일치 또는 PCI 읽기 실패 | 어떤 상태든 | **UNKNOWN** | 로그·사용자 선택; 자동 재부팅 금지 |
| `DTbtSsdt`·`AdlS_Tbt`·`AdlS_Rvp` 모두 없음 | 어떤 상태든 | **UNKNOWN** | 환경 변화/미지원 BIOS 가능성 확인 |

`DEGRADED`는 **이 시스템의 기존 Good 기준인 8 GT/s x4에는 못 미치지만 링크가 성립한 상태**를 별도로 취급한다. Thunderbolt 기능이 반드시 실패했다는 뜻은 아니다.

### 오검출 최소화를 위한 권장 순서

```text
UEFI 앱 시작 (DXE 완료 후, Windows Boot Manager 이전)
   │
   ├─ [A] 대상 보드·Root Port(0000:00:1c.4, 8086:7ABC) 확인
   │      └─ 불일치/조회 실패: UNKNOWN
   │
   ├─ [B] PCIe Capability (ID 0x10) 탐색
   │      └─ 발견 실패: UNKNOWN
   │
   ├─ [C] Link Status 2~3회 읽기 (예: 100~200ms 간격)
   │      └─ 재훈련 중이라면 잠깐 기다려 최종 상태를 측정
   │
   ├─ [D] ACPI SSDT OEM Table ID 열거
   │      └─ 테이블/헤더 검증 실패: UNKNOWN
   │
   └─ [E] ACPI + PCIe 조합으로 상태 판정
          ├─ PASS     → Windows Boot Manager 실행
          ├─ FAIL     → 재시도 제한 확인 → Warm Reset 또는 수동 선택
          ├─ DEGRADED → 경고/사용자 선택
          └─ UNKNOWN  → 자동 재부팅하지 않음
```

PCIe Link Status는 읽는 순간 바뀔 수 있으므로 **짧은 재측정**은 유용하다. 반대로 ACPI 테이블이 이미 `AdlS_Rvp`로 확정되었다면 EFI 앱에서 단순히 10초 기다린다고 PEI/DXE Thunderbolt 작업이 다시 실행되는 것은 아니다. **EFI 앱에서 PEI mailbox(최대 약 5초의 응답 polling)를 재실행하거나 지연시키려 하지 말 것.**

## 5. EFI 애플리케이션 구현 인터페이스

### 5.1 PCIe 읽기(쓰기 금지)

EDK II를 사용하는 경우 다음 API를 우선 고려한다.

- `gBS->LocateHandleBuffer(ByProtocol, &gEfiPciIoProtocolGuid, ...)`로 PCI 장치 핸들 열거.
- `HandleProtocol(..., &gEfiPciIoProtocolGuid, ...)` → `EFI_PCI_IO_PROTOCOL`.
- `PciIo->GetLocation()`으로 Segment/Bus/Device/Function을 확인해 `0000:00:1c.4` 선택.
- `PciIo->Pci.Read(PciIo, EfiPciIoWidthUint16, Offset, 1, &Value)`로 설정 공간 읽기.
- `PciIo->Pci.Read(..., EfiPciIoWidthUint8, Offset, 1, &Value)`로 Capability List 탐색.
- UEFI 펌웨어가 Root Port에 `EFI_PCI_IO_PROTOCOL` 핸들을 노출하지 않으면, 같은 BDF에 대해 `EFI_PCI_ROOT_BRIDGE_IO_PROTOCOL.Pci.Read()` 대체 경로를 구현한다. 이때 주소 인코딩은 Root Bridge I/O 규격에 맞게 해야 한다.

Capability 탐색 순서(일반 PCI Capability List):

```text
Read PCI Status @ 0x06; bit 4(Capabilities List) 확인
Read Capabilities Pointer @ 0x34
for each capability pointer (상한을 둬서 무한 루프 방지):
    cap_id = Read8(ptr + 0)
    next   = Read8(ptr + 1)
    if cap_id == 0x10: pcie_cap = ptr; break
    ptr = next
```

- Root Port의 Vendor ID가 `0xFFFF`이거나 접근에 실패하면 `UNKNOWN`.
- 헤더가 type-1 PCI-to-PCI bridge인지, PCIe Capability가 실제로 존재하는지 확인한다.
- 임의의 `Pci.Write()`, Link Disable 토글, Retrain Link, Target Link Speed 변경을 **하지 않는다**.
- 장치가 안 보이는 Thunderbolt downstream BDF를 직접 열거하는 방법은 실패 시 버스 번호가 바뀔 수 있으므로 기본 판별법으로 쓰지 않는다.

### 5.2 ACPI 읽기(쓰기 금지)

- `EFI_ACPI_SDT_PROTOCOL.GetAcpiTable()` 우선.
- `Header.Length >= 36`, `Signature`, `OemTableId`, checksum 검증.
- 테이블 길이와 사용자 측 캡처가 다른 경우 단순히 실패로 보지 말고 Baseline 불일치 기록.
- ACPI 테이블 삽입·삭제, `_OSI`·AML 수정, `InstallAcpiTable` 호출은 판별 목적에 불필요하다.

### 5.3 판정 로직 의사코드

```c
// 이 코드는 설계 의사코드다. 오류 처리·프로토콜 선언 등이 생략되어 그대로 빌드되지 않는다.
enum Verdict { PASS, FAIL, DEGRADED, UNKNOWN };

Verdict classify(PciSnapshot p, AcpiSnapshot a) {
    if (!p.read_ok || !p.identity_matches || !p.pcie_cap_valid ||
        !a.read_ok || !a.all_headers_valid || a.conflicting_tables) {
        return UNKNOWN;
    }

    bool good_acpi = a.dtbt_ssdt && a.adls_tbt && !a.adls_rvp;
    bool bad_acpi  = a.adls_rvp && !a.dtbt_ssdt && !a.adls_tbt;
    bool link_up   = p.dllla && !p.training && p.width == 4;
    bool good_link = link_up && p.speed == 3;

    if (good_acpi && good_link) return PASS;
    if (bad_acpi) return FAIL;
    if (good_acpi && !link_up) return FAIL;
    if (good_acpi && link_up && p.speed != 3) return DEGRADED;

    return UNKNOWN; // 미지원 펌웨어 / 불완전하거나 모순된 상태
}
```

## 6. 선택 기능 — 실패 시 자동 재부팅

**판별 로직과 재부팅 로직을 분리**하는 것이 좋다. 먼저 판별 전용 EFI 앱을 만들어 기존 Good/Bad 부팅에서 로그를 확인한 뒤, 필요할 때 자동 재시도를 켠다.

### 권장 실패 처리

1. `PASS` → 기록을 남기고 기존 Windows Boot Manager를 `LoadImage()` / `StartImage()`로 실행한다. 애플리케이션 자기 자신을 다시 실행하지 않도록 정확한 대상 경로(`\\EFI\\Microsoft\\Boot\\bootmgfw.efi`) 또는 기존 부팅 엔트리를 확인한다.
2. `FAIL` → 부팅 간 유지되는 **재시도 카운터**를 1 증가. 기본 `MAX_RETRY=5`를 권장한다(사용자가 설정 가능).
3. 카운터가 한도 이하이고 자동 재시도가 활성화되어 있을 때만 `gRT->ResetSystem(EfiResetWarm, ...)` 호출. 이 보드에서 Warm Reset이 Thunderbolt 초기화를 개선한다는 확증은 없으며, 실제 플랫폼 구현에 따라 reset 효과가 다를 수 있다.
4. 최대 횟수에 도달하면 **자동 재부팅 중단**. 화면에 상태/횟수를 표시하고 `Windows 그대로 부팅`, `UEFI Setup`, `종료`, `수동 재시도` 등 사용자 선택을 제공한다. 부팅이 영구적으로 막히지 않아야 한다.
5. `UNKNOWN`과 `DEGRADED`는 기본적으로 자동 재부팅하지 않는다. `PASS`이면 카운터를 초기화한다.

재시도 카운터 저장소는 **ESP 파일**(애플리케이션 전용 상태 파일) 또는 **전용 UEFI 변수** 중 하나를 선택한다. NVRAM 변수는 지속적인 쓰기를 발생시키고 펌웨어 저장소를 사용하므로, 기존 BIOS Setup 변수를 건드리지 않는 설계를 원한다면 ESP 파일이 더 명확하다. ESP 접근이 실패하거나 저장 상태가 유효하지 않으면 **무한 자동 재부팅하지 않고 수동 선택으로 빠져야 한다**. 파일/변수를 사용하는 재시도 모드는 읽기 전용 판별 모드와 달리 **상태 저장 쓰기**가 발생함을 명확히 한다.

Secure Boot가 활성화된 환경에서는 **신뢰된 서명이 없는 EFI 앱은 실행이 거부될 수 있다.** 실제 배포 단계에서 서명 및 신뢰 체인을 검토한다. 이 문서는 Secure Boot를 끄거나 펌웨어를 패치하는 것을 전제로 하지 않는다.

## 7. 검증 체크리스트와 테스트 벡터

앱을 재부팅 자동화에 사용하기 **전에** 아래 기준을 테스트한다.

| 테스트 | ACPI 입력 | PCIe Link Status | 기대 결과 |
|---|---|---|---|
| **실제 Good 스냅샷** | `DTbtSsdt=1`, `AdlS_Tbt=1`, `AdlS_Rvp=0` | `0x3043` | **PASS** |
| **실제 Bad 스냅샷** | `DTbtSsdt=0`, `AdlS_Tbt=0`, `AdlS_Rvp=1` | `0x1801` | **FAIL** |
| 링크만 장애 | Good ACPI | `0x1801` | **FAIL** |
| DXE 테이블만 장애 | Bad ACPI | `0x3043` | **FAIL** |
| 링크가 느리지만 살아 있음 | Good ACPI | Gen1/2 x4, DLLLA=1, Training=0 | **DEGRADED** |
| ACPI 조회 오류 | 미확인 | `0x3043` | **UNKNOWN** |
| Root Port Vendor/Device ID 불일치 | Good ACPI | 어떠한 상태든 | **UNKNOWN** |
| ACPI Good/Bad 마커 공존 | 모순 | 어떠한 상태든 | **UNKNOWN** |
| 관련 ACPI 마커 전부 없음 | 없음 | `0x3043` | **UNKNOWN** |

추가 확인 사항:

- [ ] BIOS 3101 Good 로그에서 Root Port가 `8086:7ABC`, PCIe Cap가 `0x40`으로 표시되는가?
- [ ] `0x3043`과 `0x1801`을 올바르게 디코딩하는가?
- [ ] `SSDT1` 등의 파일명/순번이 아닌 **OEM Table ID**를 검사하는가?
- [ ] `TbtTypeC`만 존재해도 성공이라고 오판하지 않는가?
- [ ] EFI 앱에서 ACPI 프로토콜 접근 실패 시 재부팅을 계속하지 않는가?
- [ ] PCI/ACPI에 어떤 쓰기도 수행하지 않는가?
- [ ] 자동 재시도 횟수가 영구 저장·복원되고, 저장 실패 시 무한 재부팅하지 않는가?
- [ ] 마지막 실패 후에도 Windows로 부팅하거나 EFI 설정에 들어갈 수 있는가?
- [ ] 성공 시 기존 Windows Boot Manager를 정상적으로 실행하는가?

## 8. 펌웨어 역공학으로 확인된 배경과 해석의 한계

1. `OpenBoardPlatformInitPostMem` (FFS GUID `314ee04c-1106-4dc6-acbc-cf19c0dbc5cc`)의 EndOfPei 콜백은 Thunderbolt 정책 PPI가 활성화되어 있으면 task dispatcher를 실행한다.
2. `DTbtSLCommandExecution` (PEI TE RVA `0x4501`)은 PCIe 경유 mailbox 상태를 확인하고, 실패 시 `EFI_ABORTED`를 반환할 수 있다. `0x50C8` 주변 코드에서는 PCIe 구성 공간 읽기가 `0xFFFFFFFF`를 반환하면 **최대 약 5초의 mailbox 응답 polling을 기다리지 않고 실패**하는 분기가 있다.
3. 앞선 작업 실패 시 dispatcher는 `Pass_DTbt_Policy_To_Hob` (RVA `0x47C2`) 작업 전에 중단할 수 있다. `DTbtDxe` (RVA `0xD88`)는 관련 GUID HOB가 없으면 정상 초기화 경로로 들어가지 않고 `AdlS_Tbt` → `AdlS_Rvp` 값을 선택하는 분기를 가진다.
4. 이것은 **Good/Bad의 PCIe·ACPI 차이를 설명하는 코드 수준의 가능한 인과 경로**다. Bad 부팅 중 실제로 mailbox가 어떤 EFI_STATUS를 반환했는지는 **직접 관측되지 않았다**. 따라서 이 EFI 앱은 실제 하드웨어/ACPI 상태를 판정하는 것이지, PEI 내부 실패 원인을 직접 증명하는 도구는 아니다.
5. 이전에 **Good으로 분류된 부팅에서 나중에 링크 재훈련/장애가 관측된 사례**가 있으므로, EFI 시점의 `PASS`가 Windows 실행 내내 링크를 보장하지는 않는다.

## 9. 구현 우선순위

**1단계 — 판별 전용(권장)**: 화면에 `BDF`, `Vendor:Device`, `PCIe Cap`, `LnkSta`, 속도·폭·DLLLA·Training, SSDT OEM Table ID 목록, 최종 `PASS/FAIL/DEGRADED/UNKNOWN`을 출력한다. 어떠한 자동 재부팅도 수행하지 않는다.

**2단계 — 체인로딩**: `PASS` 시 정상 Windows Boot Manager 실행. 판별 실패 시에도 사용자가 강제 진행할 수 있어야 한다.

**3단계 — 제한된 자동 재시도**: `FAIL`일 때만 Warm Reset, `MAX_RETRY`와 복구 메뉴, 상태 저장 실패 처리 추가. 반복 재부팅 성공률은 나중에 따로 검증한다.

---

### 사용한 자료

- `tbt-acpi-good-20261101-181811.zip` — Good ACPI 테이블과 `pci/0000_00_1c_4_config_first256.bin`.
- `tbt-acpi-bad-20261101-182436.zip` — Bad ACPI 테이블과 동일 경로의 PCI 구성 공간 덤프.
- `tbt_hob_re_evidence/BIOS3101_TBT_HOB_후속_정적분석.md` 및 관련 선택 함수 어셈블리.
- `ThunderboltEX4_ACPI_GoodBad_20261102.md` — ACPI 스냅샷 비교 보고서.
- [UEFI Specification 2.10 — PCI I/O Protocol](https://uefi.org/specs/UEFI/2.10/14_Protocols_PCI_Bus_Support.html)
- [UEFI PI Specification 1.9 — ACPI SDT Protocol](https://uefi.org/specs/PI/1.9/V5_ACPI_System_Desc_Table_Protocol.html)

**적용 범위**: 이 판별 기준은 **ASUS Z690-A BIOS 3101 + ThunderboltEX 4의 관측된 Good/Bad 패턴**에 맞춘 것이다. 펌웨어 업데이트·배선 변경·포트 변경 이후에는 동일 신호가 여전히 성공/실패를 구분하는지 재검증해야 한다.
