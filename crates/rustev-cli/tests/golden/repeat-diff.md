# Evaluation diff

## all

| Metric | Baseline | Run | Delta | Baseline interval | Run interval | Overlap | Within noise |
| --- | --- | --- | --- | --- | --- | --- | --- |
| coverage | 1.0 | 1.0 | 0.0 | [1.0, 1.0] | [1.0, 1.0] | true | true |
| acceptance\_coverage | 1.0 | 1.0 | 0.0 | [1.0, 1.0] | [1.0, 1.0] | true | true |
| labeled\_proposal\_coverage | 1.0 | 1.0 | 0.0 | [1.0, 1.0] | [1.0, 1.0] | true | true |
| error\_among\_accepted | 0.0 | 0.0 | 0.0 | [0.0, 0.0] | [0.0, 0.0] | true | true |
| mean\_log\_loss | 0.5 | 0.5 | 0.0 | [0.5, 0.5] | [0.25, 0.75] | true | true |
| latency\_ms/p0.5 | 10.0 | 10.0 | 0.0 | [10.0, 10.0] | [10.0, 10.0] | true | true |
| cost\_per\_1000/observed | unknown: zero denominator | unknown: zero denominator | unknown: baseline: zero denominator | unknown: 1000 undefined resamples | unknown: 1000 undefined resamples | unknown: baseline interval: 1000 undefined resamples | unknown: baseline: zero denominator |
| cost\_per\_1000/estimated | unknown: zero denominator | unknown: zero denominator | unknown: baseline: zero denominator | unknown: 1000 undefined resamples | unknown: 1000 undefined resamples | unknown: baseline interval: 1000 undefined resamples | unknown: baseline: zero denominator |

## slice

| Metric | Baseline | Run | Delta | Baseline interval | Run interval | Overlap | Within noise |
| --- | --- | --- | --- | --- | --- | --- | --- |
| coverage | 1.0 | 1.0 | 0.0 | [1.0, 1.0] | [1.0, 1.0] | true | true |
| acceptance\_coverage | 1.0 | 1.0 | 0.0 | [1.0, 1.0] | [1.0, 1.0] | true | true |
| labeled\_proposal\_coverage | 1.0 | 1.0 | 0.0 | [1.0, 1.0] | [1.0, 1.0] | true | true |
| error\_among\_accepted | 0.0 | 0.0 | 0.0 | [0.0, 0.0] | [0.0, 0.0] | true | true |
| mean\_log\_loss | 0.5 | 0.75 | 0.25 | [0.5, 0.5] | [0.75, 0.75] | false | false |
| latency\_ms/p0.5 | 10.0 | 10.0 | 0.0 | [10.0, 10.0] | [10.0, 10.0] | true | true |
| cost\_per\_1000/observed | unknown: zero denominator | unknown: zero denominator | unknown: baseline: zero denominator | unknown: 1000 undefined resamples | unknown: 1000 undefined resamples | unknown: baseline interval: 1000 undefined resamples | unknown: baseline: zero denominator |
| cost\_per\_1000/estimated | unknown: zero denominator | unknown: zero denominator | unknown: baseline: zero denominator | unknown: 1000 undefined resamples | unknown: 1000 undefined resamples | unknown: baseline interval: 1000 undefined resamples | unknown: baseline: zero denominator |

## Changed cases

Total: 0; truncated: false

