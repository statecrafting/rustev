# Explanation: `"models"`

| Side | Form | Decision | Plan | Capture |
| --- | --- | --- | --- | --- |
| a | record | `"explain-lodging"` | `"sha256:c824b35af4f831287b3fb4b346f0c7ee07c1ab7061a7971fbfa44ba385db4d4f"` | `null` |

The record lists model requests, attempts and costs.

| Claim | Value | Derivation | Lineage | Cites |
| --- | --- | --- | --- | --- |
| request | `{"instance":[],"result":{"output":{"target":0}},"step":"trip_intent"}` | `null` | `null` | `["record:/requests/0"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/trip_intent//1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":2}},"charge":{...` | `null` | `null` | `["record:/requests/0/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/0/transitions"]` |
| request | `{"instance":["c-1","k-1"],"result":{"output":{"target":0}},"step":"supports_preference"}` | `null` | `null` | `["record:/requests/1"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/supports_preference/c-1,k-1/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units"...` | `null` | `null` | `["record:/requests/1/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/1/transitions"]` |
| request | `{"instance":["c-2","k-1"],"result":{"output":{"target":0}},"step":"supports_preference"}` | `null` | `null` | `["record:/requests/2"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/supports_preference/c-2,k-1/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units"...` | `null` | `null` | `["record:/requests/2/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/2/transitions"]` |
| request | `{"instance":["c-3","k-1"],"result":{"output":{"target":0}},"step":"supports_preference"}` | `null` | `null` | `["record:/requests/3"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/supports_preference/c-3,k-1/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units"...` | `null` | `null` | `["record:/requests/3/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/3/transitions"]` |
| request | `{"instance":["c-4","k-1"],"result":{"output":{"target":0}},"step":"supports_preference"}` | `null` | `null` | `["record:/requests/4"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/supports_preference/c-4,k-1/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units"...` | `null` | `null` | `["record:/requests/4/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/4/transitions"]` |
| request | `{"instance":["c-5","k-1"],"result":{"output":{"target":0}},"step":"supports_preference"}` | `null` | `null` | `["record:/requests/5"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/supports_preference/c-5,k-1/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units"...` | `null` | `null` | `["record:/requests/5/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/5/transitions"]` |
| request | `{"instance":["c-1"],"result":{"output":{"target":0}},"step":"suitability"}` | `null` | `null` | `["record:/requests/6"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/suitability/c-1/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":2}},"charge...` | `null` | `null` | `["record:/requests/6/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/6/transitions"]` |
| request | `{"instance":["c-2"],"result":{"output":{"target":0}},"step":"suitability"}` | `null` | `null` | `["record:/requests/7"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/suitability/c-2/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":2}},"charge...` | `null` | `null` | `["record:/requests/7/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/7/transitions"]` |
| request | `{"instance":["c-3"],"result":{"output":{"target":0}},"step":"suitability"}` | `null` | `null` | `["record:/requests/8"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/suitability/c-3/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":2}},"charge...` | `null` | `null` | `["record:/requests/8/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/8/transitions"]` |
| request | `{"instance":["c-4"],"result":{"output":{"target":0}},"step":"suitability"}` | `null` | `null` | `["record:/requests/9"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/suitability/c-4/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":2}},"charge...` | `null` | `null` | `["record:/requests/9/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/9/transitions"]` |
| request | `{"instance":["c-5"],"result":{"output":{"target":0}},"step":"suitability"}` | `null` | `null` | `["record:/requests/10"]` |
| attempt | `{"artifact":"sha256:3374000791e29cca4b8625e4c6f453e338c073becc0b8d55df31aea6796ca0ec","attempt_id":"explain-lodging/suitability/c-5/1","backend_id":"synthetic-rules-lodging","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":2}},"charge...` | `null` | `null` | `["record:/requests/10/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/10/transitions"]` |
| served_identity | not available: not recorded (facet served_identity) | `null` | `null` | `[]` |
| backend_cost | `{"estimated":0,"observed":22,"unknown":0}` | `null` | `null` | `["record:/requests/0/attempts/0/cost/charge","record:/requests/1/attempts/0/cost/charge","record:/requests/10/attempts/0/cost/charge","record:/requests/2/attempts/0/cost/charge","record:/requests/3/attempts/0/cost/charge","record:/requests/4/attempts/0/cos...` |
| cost_summary | `{"bound_violations":0,"estimated":0,"final_cost":"known","liability":0,"limit":0,"mode":"unlimited","observed":22,"within_guaranteed_cap":false}` | `null` | `null` | `["record:/cost"]` |
