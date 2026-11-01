# TbtRetry

**UEFI 기반 Thunderbolt 초기화 어플리케이션**

Thunderbolt 초기화가 실패하는 특정 메인보드를 위한 우회책입니다.

**Thunderbolt가 인식되지 않았다면, 인식될 때까지 재부팅합니다.**

## Overview

TbtRetry는 일부 메인보드에서 발생하는 간헐적인 Thunderbolt 초기화 실패를 우회하기 위한 경량 UEFI 애플리케이션입니다.

부팅 과정에서 BIOS가 생성한 ACPI 테이블을 검사하여 Thunderbolt 초기화 성공 여부를 판별합니다.

- **초기화 성공:** 다음 EFI 애플리케이션 또는 운영체제 부트로더 실행
- **초기화 실패:** 시스템을 자동으로 재부팅
- **재시도 횟수 초과:** 무한 재부팅을 방지하기 위해 재시도 중단

운영체제가 시작되기 전에 실행되므로 Windows나 Linux 드라이버에 의존하지 않습니다.

## Background

ASUS ROG STRIX Z690-A GAMING WIFI D4와 ThunderboltEX 4 조합에서 간헐적인 장치 초기화 실패가 관측되었습니다.

이 문제는 다음과 같은 특징을 보입니다.

- 동일한 BIOS 및 하드웨어 구성에서도 부팅할 때마다 결과가 달라집니다.
- 실패 시 Thunderbolt PCIe 장치가 열거되지 않습니다.
- PCIe 링크 재훈련 및 Link Disable 토글로는 복구되지 않았습니다.
- S5/G3 전원 상태 전환만으로는 성공이 보장되지 않습니다.
- Warm Reboot를 반복하면 정상 인식되는 경우가 확인되었습니다.

### ACPI Investigation

BIOS 3101에서 정상 부팅과 실패 부팅의 ACPI 테이블을 비교한 결과, 다음과 같은 차이가 발견되었습니다.

| ACPI Table | Success | Failure |
|---|---|---|
| `DTbtSsdt` | Present | Missing |
| `AdlS_Tbt` | Present | Missing |
| `AdlS_Rvp` | Missing | Present |
| `TbtTypeC` | Present | Present |

정상 부팅에서는 Thunderbolt 관련 SSDT가 설치되고 PCIe 링크가 8 GT/s ×4로 활성화되었습니다.

반면 실패 부팅에서는 해당 SSDT가 누락되었으며 PCIe 링크가 비활성 상태로 남았습니다.

이러한 관측 결과를 기반으로 TbtRetry는 ACPI 테이블을 초기화 성공 여부의 판단 지표로 사용합니다.

**Note:** ACPI 테이블의 차이는 초기화 결과와 연관된 관측 증거이며, 문제의 근본 원인이 ACPI 자체라는 의미는 아닙니다.

## How It Works

```text
UEFI Firmware
      |
      v
  TbtRetry.efi
      |
      v
 Scan ACPI Tables
      |
      v
 Thunderbolt Ready?
      |
      +---- YES ----> Next EFI Application
      |                    |
      |                    v
      |              Windows / Linux
      |
      +---- NO -----> Reboot
                           |
                           v
                      UEFI Firmware
                           |
                           v
                      TbtRetry.efi
```

TbtRetry는 장치의 펌웨어를 수정하거나 PCIe 링크를 강제로 복구하지 않습니다.

대신 BIOS 초기화 결과를 확인하고, 정상적인 결과를 얻을 때까지 초기화 과정을 반복합니다.

## Standalone EFI

같은 파티션의 `\EFI\BOOT\`에 다른 EFI가 없으면 `boot_name` 설정과 관계없이 감지·재시도를 수행한 뒤 `EFI_ABORTED`를 반환하여 펌웨어가 다음 부팅 항목을 시도하게 합니다. 재시도 카운터 초기화와 디버그 성공 로그 기록도 수행합니다. 다른 EFI가 있으면 기존처럼 체인로딩합니다.

단독 구성에서는 펌웨어 부팅 순서를 **TbtRetry → OS 부트로더**로 설정하세요. 펌웨어 부팅 관리자가 TbtRetry를 부팅 항목으로 직접 실행하는 구성을 전제로 합니다.

`tbt-retry.toml`에 `use_boot_order = true`를 설정하면 다른 EFI가 있어도 파일 탐색·선택·실행을 건너뛰고 펌웨어 부팅 순서에 맡깁니다. 기본값은 `false`이며 기존 설정 파일에 옵션이 없어도 동작합니다. 이 옵션은 `boot_name`의 대상 선택보다 우선하지만, 설정 형식 검증은 유지합니다.

두 경우 모두 초기화 실패 시 재부팅 재시도를 먼저 수행합니다. 성공 또는 한도 도달 후에만 다음 단계로 넘어가며, 감지 불명·카운터 오류 시에는 기존처럼 재시도를 중단하고 부팅을 이어갑니다. `EFI_ABORTED`는 이 마지막 단계에서 다음 부팅 항목을 시도하도록 반환하는 상태이며, 재시도를 생략한다는 뜻이 아닙니다.

## Compatibility

### Tested Hardware

| Component | Configuration |
|---|---|
| Motherboard | ASUS ROG STRIX Z690-A GAMING WIFI D4 |
| Thunderbolt Card | ASUS ThunderboltEX 4 |
| BIOS | 3101 |
| Thunderbolt NVM | 36 |

현재 확인된 ACPI 테이블 식별 방식은 위 구성에 기반합니다. 다른 메인보드 및 BIOS 버전에서의 동작은 검증되지 않았습니다.

## Limitations

- Thunderbolt 초기화 실패의 근본 원인을 수정하지 않습니다.
- 재부팅 후 정상적으로 초기화될 확률은 시스템에 따라 다릅니다.
- ACPI 테이블만으로 모든 Thunderbolt 장치의 정상 동작을 보장할 수 없습니다.
- 다른 BIOS 버전에서는 ACPI 테이블 구조가 달라질 수 있습니다.
- BIOS가 매번 동일하게 초기화에 실패하는 환경에서는 효과가 없습니다.

## Why?

Race Condition을 해결하는 가장 최선의 방법이 재시도밖에 떠오르지 않았습니다.

**돌아가면 부팅, 안 돌아가면 재부팅.**

---

*Experimental UEFI firmware workaround. Not affiliated with ASUS or Intel.*
