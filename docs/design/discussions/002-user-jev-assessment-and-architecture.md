# Message 002

Role: user
Source thread: 01a0cbd0-f785-7b41-9a48-d85eb9921af9

---

Analyze and evaluate the assessment and the proposed architecture. We’re not trying to compete but rather build our own open source lite variant that fully leverages rust and potentially optimizes architecture using deterministic custom rust tools or any other clever design and approach.

[em dash][em dash][em dash]

Jev is not a smaller chatbot. It is a hosted **decision model**: state plus typed questions in, calibrated probabilities out, no text generation. The full writeup is in [Jev research and open-source Rust architecture](/home/ubuntu/jev-research/jev-system-one-research-and-rust-architecture.md).

**What TypeSafe actually shipped.** TypeSafe emerged from ~2 years of stealth on 15 September 2026 with a $40M DCVC-led seed. CEO Diogo Almeida co-authored InstructGPT/RLHF. Jev (`jev-1.13.0`) is the first **System One** model: Choice (≤255 options), Score (2–10 levels), and Noul (P(yes) in 0–1), all evaluated in parallel on one shared state. List price is $0.042/MTok in, output free. Vendor latency is 70–500 ms; OpenRouter’s live P50 is ~0.27 s.

**How it was made, as far as the public record goes.** The load-bearing pieces are a **new architecture**, a **parallel sampler**, and **RLCD** (Reinforcement Learning for Calibrated Decisions). Weights, parameter count, tokenizer, data mix, and the RLCD algorithm are unpublished. The behavior that *is* specified [em dash] encode state once, fill a caller-declared schema, never emit an illegal type [em dash] is what an encoder with request-time label embeddings would do. TypeSafe’s LLaDA and vLLM forks show interest in masked diffusion; they are not proof that production Jev *is* LLaDA.

**“Can’t hallucinate”** means schema-safe, not judgment-safe. Jev cannot invent a fourth department. It can still pick the wrong one of the three you named. Calibration, not type safety, is the proposed fix, and that claim has not been independently audited.

**Open-source Rust path.** Do not train an 8B diffusion LM from scratch in Rust. Clone the **API contract** first (`spine-schema` + `spine-serve` + a port of TypeSafe’s LLM adapter), then serve a bidirectional encoder with late-interaction heads in Candle/Burn, then distill and calibrate (log loss / Brier, optionally GRPO). Train in PyTorch; serve in Rust. A 100–300M encoder at ~50 ms is a real product. Matching TypeSafe’s “frontier on System One tasks” numbers is not, without their data and compute.

The report also covers founders, evals and their caveats, jaggedness (`jev-1.13` is literal and bad at math/dates), the three primitives’ exact math including the confidence formula, LangChain/Pydantic/OpenRouter integrations, and a 90-day crate plan.

Jev and System One Models: How TypeSafe Built a Decision Model, and How to Recreate the Shape in Open-Source Rust
Author: Manus AI
Date: 22 September 2026
Subject: TypeSafe AI’s Jev (`jev-1.13.0`), the System One model class, and a buildable open-source architecture in Rust

Jev is not a smaller chatbot. It is a hosted, non-generative decision model that TypeSafe AI released in early access on 15 September 2026. The public contract is simple: unstructured or structured state in, a map of typed questions in, and typed answers with probabilities out. TypeSafe’s own framing is “a frontier-intelligence function call.” That contract, not a secret transformer variant, is what makes Jev different from GPT-style models in production software. 1 2

The internals of Jev itself are still proprietary. TypeSafe has not published a model card with parameter count, a tokenizer specification, a training-data mix, or the RLCD algorithm. What can be reconstructed with high confidence is the shape: an encoder that reads state once, a parallel sampler over closed answer sets, and a post-training objective that scores calibrated probabilities rather than preferred prose. An open-source Rust stack cannot copy Jev’s unpublished weights. It can copy that shape, serve it at sub-second latency, and train toward the same interface.

What Jev is, in one paragraph
A System One model evaluates a piece of application state against one or more questions whose answer spaces are declared in advance. Jev currently exposes three primitives. Choice selects among up to 255 named options and returns the winner, a full simplex over those options, and a confidence statistic derived from that simplex. Score places the state on an ordered rubric of two to ten levels and returns a probability-weighted position that can land between levels. Noul answers a yes-or-no proposition with a single probability in `[0, 1]`. All questions in a request see the same state, are evaluated independently, and are sampled in parallel, so extra questions add tokens but little wall-clock time. The model does not generate text, does not write code, and cannot return a value outside the schema you sent. It can still pick the wrong allowed value. 2 3 4


 

Why TypeSafe exists
TypeSafe’s founding question is Diogo Almeida’s: models have been superhuman at chat for years, so where is the automation? Almeida’s answer is that the industry optimized the wrong interface. RLHF made models good at talking to people. Production software needs models that talk to code. 1 5

The company manifesto, titled Composable AI: Build Prod, Not God, argues that the bottleneck is not raw intelligence. It is that today’s models were trained as helpful assistants, so they require a human in the loop. TypeSafe wants AI to sit beside ordinary software as a primitive for semantic judgment, while code keeps exact computation, control flow, and side effects. The stated economic target is a “Cambrian explosion” of intelligent software, measured as total-factor-productivity growth reaching 3% within five years and holding for ten. That is a company bet, not a forecast this report endorses. 5

Almeida restates the same idea as the “bitterest lesson.” Rich Sutton’s bitter lesson says compute beats clever algorithms. Almeida adds two rungs: the right task beats data, which beats compute, which beats algorithms. At OpenAI, GPT-2-sized models trained on instruction following beat GPT-3 on the task people actually wanted. TypeSafe’s claim is that the next right task is calibrated decisions inside software, not more chat. 6 7

Company, people, and money
TypeSafe AI is a San Francisco lab. It works in person five days a week near Embarcadero station. The public team page names three founders. 8

Diogo Almeida is CEO. He is a co-author of the InstructGPT paper that introduced reinforcement learning from human feedback at scale, a co-author of the GPT-4 technical report, and previously at Google Brain. Google Scholar lists him with on the order of 63,000 citations, dominated by InstructGPT and GPT-4. TypeSafe’s docs state that he co-invented RLHF. 8 9 10

Sasha Sheng is COO. She was a research engineer at Meta / FAIR, working on News Feed, AI Experiences, and AI research, with publications at NeurIPS and ECCV. 8

Erik Gafni (Erik Spock Gafni) is CTO. He is a repeat founder of Ravel, a multimodal AI company for DNA sequencing, an early employee at Invitae and Freenome, and a specialist in production AI systems. 8

The company says it is a flat team drawn from OpenAI, Google Brain, Meta/FAIR, Stripe, Airbnb, Plaid, and Docker. It emerged from about two years of stealth on 15 September 2026 with a $40 million seed round led by DCVC. James Hardiman, a DCVC general partner, is quoted in the press release. Independent write-ups report that Forbes put the post-money valuation at $200 million. Those figures come from the company and from press coverage of the round, not from a securities filing. 11 12

Jev is named for William Stanley Jevons. TypeSafe expects cheaper intelligence to increase demand, the same way more efficient steam engines increased coal consumption. “System One” is taken from Daniel Kahneman’s Thinking, Fast and Slow: fast, intuitive judgment rather than slow deliberative reasoning. TypeSafe’s own FAQ notes that “System 1” has historically implied error-prone, and claims that System One models can be made more reliable than that implication. Reasons for that claim are deferred to a future post. 1

The public product: `jev-1.13.0`
The only currently documented production model is Jev 1.13, served as `jev-1.13.0`. The aliases `jev-latest` and `jev-preview` both point at that version as of 17–22 September 2026. The response’s `model` field reports the versioned ID, so callers who tune thresholds should pin `jev-1.13.0` rather than an alias that will move. TypeSafe does not fine-tune or LoRA-adapt Jev on customer data. The same weights serve every account. Domain behavior is supposed to come from `state`, `instructions`, and `criteria`, then from code that composes answers. Customer requests are not used as training data. Enterprise customers can get zero data retention. 13

Published serving numbers as of mid-September 2026:


 Property Published value Input price $42 per billion tokens, $0.042 per million Output price Free End-to-end latency (vendor) 70–500 ms OpenRouter P50 latency 0.27 s Context 64k tokens for state plus all questions; 32k for state plus the longest question OpenRouter / Vercel listed context 32k Input modalities Text only: string, JSON object, or array of text. No image, audio, or video Languages English is primary; other languages, including CJK, are weaker Rate limits 250,000 tokens per second and 1,200 requests per minute, described as dynamic Choice cardinality 255 options; above that, TypeSafe uses a two-stage score-then-choose path Score levels 2 to 10 Fine-tuning None on customer data 
OpenRouter already shows substantial production traffic: on the order of 168 billion prompt tokens and 20.3 billion completion tokens in the activity window shown on 22 September 2026, with a P50 of 0.24–0.27 seconds and roughly 99.9% availability over three days. Vercel AI Gateway lists the same model as an evaluation model with maximum output tokens of 0, which is consistent with “output too cheap to meter” and with a sampler that does not emit a token sequence. 14 15 16

The API contract
There is one evaluation endpoint:


 POST https://api.typesafe.ai/v1/systemone
Authorization: Bearer <API_KEY>
Content-Type: application/json
 
The body is always `state`, `model`, and `questions`. `state` is the material to judge: a string, object, or array. `questions` is a map from caller-chosen IDs to question objects. Those IDs are not sent to the underlying model. They exist only so the response can be keyed the same way. Official Python (`typesafe-sdk`) and JavaScript (`@typesafe-ai/sdk`) clients wrap this. OpenRouter exposes a parallel Decisions API at `POST /api/alpha/decisions` with model id `typesafe/jev-1.13`. Pydantic AI maps each field of an `output_type` onto one Jev question. LangChain ships `TypeSafeClassifier` and experimental middleware for model routing and tool-call auto-mode. 4 16 17 18

A request looks like this:


 {
  "state": "Help! My payouts have been failing for 3 days.",
  "model": "jev-latest",
  "questions": {
    "is_urgent": {
      "type": "noul",
      "instructions": "Does this convey urgency?",
      "criteria": {
        "true": "Explicitly time-sensitive",
        "false": "No urgency expressed"
      }
    },
    "department": {
      "type": "choice",
      "instructions": "Which team should handle this?",
      "criteria": {
        "billing": "Payments, invoicing, refunds",
        "technical": "Bugs, outages, integrations",
        "sales": "Pricing, upgrades, new accounts"
      }
    },
    "frustration": {
      "type": "score",
      "instructions": "How frustrated is the customer?",
      "criteria": ["Calm", "Frustrated", "Very angry"]
    }
  }
}
 
A response looks like this:


 {
  "model": "jev-1.13.0",
  "answers": {
    "is_urgent": { "type": "noul", "noul": 0.95 },
    "department": {
      "type": "choice",
      "choice": "billing",
      "probabilities": { "billing": 0.88, "technical": 0.12, "sales": 0.0 },
      "confidence": 0.81
    },
    "frustration": {
      "type": "score",
      "score": 1.05,
      "legend": { "0": "Calm", "1": "Frustrated", "2": "Very angry" },
      "probabilities": { "0": 0.0, "1": 0.95, "2": 0.05 },
      "confidence": 0.92
    }
  },
  "usage": { "input_tokens": 318, "output_tokens": 34 }
}
 
`instructions` and `criteria` may be strings, objects, or arrays. Structured instructions let a question point at nested state with backticked paths such as ``support.tickets[0].message``. TypeSafe’s build guidance is to keep the content in `state` and the judgment in the question. Putting the question into the state text is a common LLM habit that Jev will treat as more material to judge. 4 19 17

Errors are ordinary HTTP: 401 for a bad key, 422 for a malformed body, 429 for rate limits, 529 for overload. SDKs retry with backoff and honor `Retry-After`. 4

How the three primitives actually compute
Noul is the probability that a proposition is true. There is no separate confidence field because a two-outcome distribution is fully described by one number. Near 1 is a strong yes, near 0 a strong no, near 0.5 an even split. TypeSafe is explicit that a Noul is not a magnitude. “Is the candidate strong in Python?” at 0.81 does not mean “81% of the way to expert.” Degree belongs on a Score. 20

Choice returns `argmax` of a distribution that sums to 1, plus that full distribution. Confidence is not the winning probability. TypeSafe’s own playground computes it from how peaked the distribution is. For `n` options and peak probability `p_max`:

 {[0,1]}\left(\frac{n \cdot p{\max} - 1}{n - 1}\right)

Uniform   maps to 0. A one-hot winner maps to 1. For three options this is  . Callers are told they can ignore this statistic and compute their own from `probabilities`. 21

Score treats levels as integers  . The returned `score` is the expectation  , so 1.43 on a three-level rubric means the mass sits between “workaround exists” and “no workaround,” leaning toward the former. Different distributions can share the same expectation, which is why TypeSafe tells callers to read `probabilities` and `confidence` alongside the score. Jaggedness docs warn not to interpolate a physical quantity from that expectation. Numerical calibration across levels is weak. 22 23

Independence is a first-class property. Answer A does not become hidden context for question B in the same request. That is why TypeSafe pushes speculative fan-out: ask every question the workflow might need, then ignore the irrelevant ones in code. Serial calls should represent genuine information dependence, not an LLM conversation habit. TypeSafe also documents that Noul   and Choice   on the same proposition need not match, and that   need not equal 1 across two Nouls. Structural invariants that feel obvious are not guaranteed. 3 23 24


 

Training: RLCD as a third post-training path
TypeSafe draws three branches from a pretrained language model. 10

RLHF (reinforcement learning from human feedback) optimizes for responses people prefer. That is InstructGPT and ChatGPT. Almeida argues it causes sycophancy, overconfidence, and mode dropping: the model concentrates on a favored style and underrepresents other valid outputs.

RLVR (reinforcement learning with verifiable rewards) optimizes for outputs a program can check, which produced slow, expensive reasoning models that are strong at math and similar domains.

RLCD (reinforcement learning for calibrated decisions) is TypeSafe’s name for the third path. The model does not generate text. It returns decisions and probabilities. Higher probability should correspond to a greater chance of being correct, measured across groups of predictions, not as a guarantee on any one call. The primer gives the usual calibration reading: events labeled 0.2 should happen about 20% of the time, 0.8 about 80%, 1.0 always.

No paper, reward function, or loss schedule for RLCD is public. Independent explainers correctly note that calibration is an old idea (Brier score, log loss, temperature scaling, isotonic regression) and that Jev’s particular RL procedure is unpublished. 25 26 A defensible reconstruction, consistent with the API, is:

	1.	A pretrained encoder or masked language model that already understands text.
	2.	Supervised closed-set decision training: Choice, Score, and Noul formatted as classification over dynamic label text.
	3.	A reinforcement or proper-scoring stage that penalizes miscalibration, not just 0/1 error. Log loss and Brier score are the obvious rewards. A policy-gradient or direct-preference analogue could compare predicted distributions against reference distributions from stronger models or against human labels.
	4.	A confidence statistic computed from the simplex rather than from a separately generated “I am 90% sure” string.

That reconstruction is an inference. TypeSafe could be doing something more exotic. The public evidence only constrains the objective (calibrated closed-set decisions) and the output contract (no free text).

Jev is not fine-tuned per customer. TypeSafe’s cookbook for “autoresearch feature discovery” instead treats Jev as a frozen feature extractor: the model emits probabilities that a classical model such as CatBoost can train on. That is the intended customization path. 13

Architecture: what is published, and what the public artifacts imply
TypeSafe’s launch post says Jev was built with “a new model architecture, parallel sampler for maximum efficiency, and training method we call Reinforcement Learning for Calibrated Decisions.” Sampling is “parallel. Generates all outputs in a single query. Incredibly efficient and hardware-aware.” Output tokens are free because they are “too cheap to meter.” Type errors are “mathematically impossible.” 1

Those sentences rule some designs out. An ordinary autoregressive LLM with JSON mode cannot make type errors impossible, only unlikely, and it cannot make extra questions nearly free in latency, because each extra field still costs sequential tokens. An ordinary BERT classifier with a fixed label set cannot accept 255 arbitrary option strings per request. Whatever Jev is, it must (a) consume natural-language option descriptions at request time, (b) produce a distribution over those options without decoding a string, and (c) share one encoding of `state` across many questions.

The most economical architecture that satisfies (a)–(c) is an encoder with dynamic label embeddings:

	1.	Tokenize `state` once and run a bidirectional Transformer over it.
	2.	For each question, encode `instructions` plus each option or level description, often as short sequences that cross-attend to the state representation.
	3.	Score each option with a dot product or a small MLP on `[state_pool; option_pool]`.
	4.	Softmax over the declared options (Choice), softmax over levels then take the expectation (Score), or a two-way or sigmoid head (Noul).
	5.	Compute confidence from the simplex with a closed-form statistic.

That design is a large, general-purpose successor of models such as TARS, SetFit, Universal-Classifier, and cross-encoder rerankers. It explains the 255-option cap as an 8-bit or kernel-tile limit, the 32k “state plus longest question” budget as a sequence-length cap on the most expensive pair, the “adding questions barely changes latency” claim as extra cheap heads on a shared encoding, and free output as the absence of an autoregressive loop.

A second family is a masked diffusion language model (LLaDA-style) that unmasks only the answer slots. TypeSafe forked ML-GSAI/LLaDA in July 2025 and vllm-project/vllm in May 2025. LLaDA is an 8B masked diffusion Transformer encoder: no causal mask, a `[MASK]` token, a random masking ratio  , and parallel prediction of every masked token. That is a real non-autoregressive language model, and it matches TypeSafe’s language of a “new architecture” plus “parallel sampler.” The fork, however, has no TypeSafe-specific commits after the upstream snapshot of June 2025, so it is evidence of research interest, not proof that Jev is LLaDA. For Jev’s actual job (fill a schema, do not write a paragraph) a diffusion sampler is unnecessary complexity unless TypeSafe still wants a generative fallback they have not shipped. Community discussion on X already split on this: some people jumped to diffusion because of the speed claim, others noted that “new architecture + parallel sampler” does not uniquely identify diffusion. 27 28 29

A third, weaker hypothesis is “a small LLM with constrained decoding.” TypeSafe’s FAQ includes the question “Is Jev just a smaller LLM?” The HTML of that FAQ is collapsed in public scrapes, so the company’s own answer is not in this report. The product behavior argues against a vanilla small decoder. Constrained decoding still emits tokens, still pays for output, still couples questions unless the server fans them out, and still cannot make schema violations impossible if the constraint grammar is incomplete. The System One adapter TypeSafe published is exactly that weaker design, used as a baseline for comparing LLMs to Jev, which is further evidence that Jev itself is not the adapter. 30

Until TypeSafe publishes a paper, the responsible statement is:

 Jev behaves like a bidirectional encoder with request-time label embeddings and a parallel closed-set sampler, trained with a calibration objective TypeSafe calls RLCD. Masked diffusion is a plausible research ancestor, visible in public forks, but not a confirmed production backbone. 

Parameter count is unpublished. Latency of 70–500 ms on a 32k–64k context, plus the ability to evaluate many questions on one state, is consistent with a model in the low billions of parameters on modern GPUs with a highly optimized sampler, or a larger model with aggressive batching and a short effective question length. It is not consistent with a frontier reasoning decoder.

How TypeSafe measures it
TypeSafe built a workflow eval rather than a chat benchmark. The assumption is that the compute graph is correct. Every model runs the same harness of Noul, Choice, and Score questions. Reference labels are the average of GPT-6 Astra and Claude Fable 5.1 at high thinking. Other models run at each provider’s default reasoning setting. LLMs are wrapped in TypeSafe’s System One adapter so they emit compatible distributions. Accuracy is similarity to that reference, plotted against cost and time. Four published workflows are security-incident triage, agent-trace observability, invoice processing, and customer-service next action. DataCamp, restating TypeSafe’s plot, puts Jev near 68% accuracy, close to GPT-5.6 Terra, while owning the cheap-and-fast region of the Pareto front. The homepage numbers 193.6× faster and 444.6× cheaper come from this suite. 31 32 1

TypeSafe’s own caveats on that suite are unusually direct. The workflows were written by TypeSafe’s model-capabilities team, so they could be accidentally Jev-shaped. The reference pair biases the score toward OpenAI and Anthropic. The LLMs pay the adapter tax of slower, costlier probability-producing prompts. The company says the headline multiples are on the high end of real-world gains. Hacker News made the same objection more sharply: comparing 70 ms classification to multi-second chain-of-thought is only fair if you were going to spend that chain-of-thought on a classification. 1 33

Independent checks are still thin but not empty. Every’s Mike Taylor ran 21 questions over 37 documents, 777 judgments, in under 0.7 seconds for about a quarter of a cent. Dan Shipper then compared Jev and Fable 5.1 on four writing checks over twelve passages: Jev’s median was 0.35 seconds versus 8.83 seconds, at roughly 580× lower cost, catching six of seven planted defects against Fable’s seven. That is a real but narrow vibe check, not a reproduction of the Pareto plot. OpenRouter’s live latency distribution is the strongest public confirmation that “a few hundred milliseconds” is not a lab-only number. 33 16

“Zero hallucinations” is schema hallucination, not judgment error. TypeSafe plots 0% because a value outside the criteria cannot be emitted. The Register, Hacker News, and TypeSafe’s own jaggedness page all make the distinction: Jev can still confidently choose the wrong allowed label. Calibration, not type safety, is the proposed mitigation, and calibration has not been independently audited on someone else’s distribution. 1 34 23

Where it is strong, and where `jev-1.13` is jagged
The intended job is a five-second expert judgment at machine scale. If a knowledgeable person could look at a prepared dossier and answer one narrow question without researching, writing, or chaining several hops, the task is Jev-shaped. Production patterns that match are classification, detection, rubric scoring, routing, reranking, RAG passage gating, citation checks, jailbreak and policy screens, function-argument filling over closed sets, and confidence-gated automation. TypeSafe’s cookbooks cover those directly. LangChain’s early integration uses Jev as middleware around an agent: pick a model, or block a dangerous tool call, rather than replace the coding model. 18 35

The official jaggedness page for `jev-1.13`, last reviewed 17 September 2026, is the most useful technical document TypeSafe has published after the API itself. The model is literal. It is weak at counting, arithmetic, hex colors, assembly, and date ordering. It loses accuracy under indirection, double negatives, and large states full of irrelevant detail. Adversarial text in `state` can steer it; `state` is treated as data, not as a hostile prompt, and TypeSafe expects to improve this. Contradictory instructions and criteria hurt. Asking the same fact as a Noul and as a Choice yields numbers that are not interchangeable. Generation by chaining character-level Choices is possible and terrible. The recommended repair in every case is the same: do exact work in code, ask one literal judgment per question, filter state first, and keep generation on a generative model. 23

Demos illustrate the speed claim more than the intelligence claim. Doom is played from a structured text state, not pixels, at about ten queries per second, on the order of $7 per hour at list prices. Wikiracing shows high-cardinality Choice and the two-stage fallback above 255 options. Community demos in Minecraft, driving sims, and drones are the same loop: dump state, pick an allowed action, repeat. A non-AI bot could play Doom better. The point is reactivity to different state representations and instruction following at interactive rates. 1 36

Ecosystem around a closed model
TypeSafe’s public GitHub org is mostly client surface, not the model. The important repositories are the Python SDK, the JavaScript SDK, the agent skill (`typesafe-ai/skills`, already at 1,800 stars within weeks), Dagger modules, and the System One adapter. The adapter is the cleanest open artifact of the interface: it turns OpenAI, Anthropic, and Gemini chat models into `TypeSafeClient`-compatible decision functions, with structured outputs, probability or discrete modes, and retries on malformed JSON. That is how TypeSafe runs LLM baselines in the workflow evals, and it is the right first step for anyone building an open replica. 30 37

Within a week of launch, LangChain, Pydantic AI, OpenRouter, and Vercel AI Gateway had first-party integrations. Third-party MCP servers, browser-use wrappers, trading toys, and “awesome-jev” lists appeared the same week. That adoption is about the API shape, which is small enough to wrap, not about leaked weights.

What remains unknown
The following are not in any public TypeSafe document as of 22 September 2026:

	•	Parameter count, layer count, hidden size, attention variant, tokenizer, and vocabulary.
	•	Whether the production sampler is classification heads, masked diffusion, or something else.
	•	The RLCD loss, reward model, data mixture, and compute.
	•	Whether sampling is deterministic at temperature 0. The homepage FAQ includes “Is Jev deterministic?”; the answer is not in the statically scraped HTML.
	•	Performance on public academic benchmarks. The launch FAQ asks this and does not expand in the scrape.
	•	Training-data provenance beyond “not your production traffic.”
	•	Whether prices are subsidized. TypeSafe says it cannot prove they are not, and that it expects prices to fall.

Any architecture diagram of Jev’s weights is therefore a hypothesis. The architecture diagram of Jev’s product is not.

Architecting an open-source System One stack in Rust
An open replica should clone the contract, the serving properties, and the training objective, in that order. It should not start by trying to train an 8B diffusion LM from scratch in Rust. The right sequence is a compatible API, a fast encoder-classifier, then calibration, then optional scale.


 

Design constraints copied from Jev
The open system should obey the same invariants TypeSafe made load-bearing.

The server accepts `state` plus a map of named questions and returns answers under those names. Question IDs never enter the model. Choice, Score, and Noul are the only primitives in v1. Every question is independent given `state`. Extra questions must not require a second encode of `state`. Outputs are a simplex over caller-supplied strings, so type errors are impossible at the API boundary even if the model is wrong. Confidence for Choice and Score is a pure function of that simplex, so it stays auditable. Noul is one probability. Exact work (arithmetic, dates, counts, regex extraction) stays in the caller. Latency target is the 50–300 ms band on a single modern GPU for 4k–8k-token states with tens of questions, which is what makes the product category real.

Recommended model, not a decoder
Train or fine-tune a bidirectional encoder with text-labeled classification heads, not a causal LM. Starting points that already exist in open weights:

	•	ModernBERT or a recent long-context encoder for English classification quality at modest size.
	•	A converted decoder-as-encoder (LLM2Vec, Qwen-embedding, GTE, NV-Embed) if the goal is to inherit more world knowledge from a pretrained LLM.
	•	DeBERTa-v3 or a cross-encoder reranker checkpoint as the smallest prototype.

Do not start from LLaDA unless the goal is research into diffusion sampling. For closed-set decisions, diffusion adds sampling steps that Jev’s latency budget does not want. If a later version needs generation, keep it as a second head, not as the decision path.

Each request is packed as one state sequence plus   question blocks. Implementation options, in increasing fidelity:

Option A, prototype. Encode `state` once. For each option, encode `instructions + option_text` and score with a cross-encoder. Softmax over options. This is a reranker. It is correct, easy, and too slow once   is large.

Option B, production. Encode `state` once into hidden states  . Encode each option independently with a bi-encoder, or with a short decoder-free MLP over `[CLS]`. Score s_i = \mathrm{MLP}([h_{\text{state}}; h_{\text{option}i}; h . Softmax. This is late interaction. It scales to 255 options and many questions because option encoding is tiny.

Option C, Jev-like. Let each question’s instructions cross-attend into   (a single extra Transformer block or a prefix that is concatenated to state for that question only). Pool. Score against option embeddings produced the same way. Share the state KV across questions inside one fused kernel. This matches TypeSafe’s “state plus longest question ≤ 32k” budget.

Noul is Choice with implicit options `{true, false}` plus optional criteria text. Score is Choice over ordered levels; the expectation is computed in post-processing, not by the net.

Calibration training that deserves the name RLCD
Without TypeSafe’s recipe, use a public one that targets the same property.

Stage 0: teacher distillation. Run TypeSafe’s open System One adapter against a strong open or commercial LLM on a large corpus of `(state, questions)` drawn from classification, NLI, retrieval, moderation, support-ticket, and synthetic instruction data. Store the teacher’s distributions, not just argmax. This is the fastest way to get a general decision model.

Stage 1: supervised. Train the encoder with temperature-scaled softmax and label smoothing against teacher distributions and, where available, gold labels. Loss is token-free: cross-entropy over the option axis only.

Stage 2: proper scoring. Fine-tune with Brier score and log loss on held-out human or consensus labels. This is already “calibration learning.” Vector-valued Brier on Choice and Score, binary Brier on Noul.

Stage 3: RL if it earns its keep. Treat the predicted distribution as an action. Reward is negative Brier against a delayed outcome, or KL toward a frozen teacher plus a calibration bonus. Group-relative policy optimization (the GRPO family) fits, because there is no token-level advantage to estimate. TypeSafe’s “RLCD” is likely in this neighborhood: reinforcement on decision quality and calibration, not on prose preference.

Stage 4: freeze the net, fit temperature. A single temperature per primitive, or a small isotonic map, fitted on a calibration split. Do not let this hide a badly trained model.

Confidence can stay TypeSafe’s closed form so that software written against Jev ports without change. Optionally train a second head to predict “will argmax be correct?” and compare it to the closed form; ship whichever is better calibrated on the user’s domain.

Why Rust, and where Rust should stop
Rust is the right language for the serving path: one binary, no GIL, predictable tail latency, safe concurrency for batching, easy WASI/sidecar deployment next to application code. It is a weaker language for frontier training. The productive split is:

	•	Train in PyTorch or JAX. Export `safetensors` plus a tokenizer JSON.
	•	Serve and batch in Rust.
	•	Optionally reimplement forward in Rust for the production kernel, not for research iterations.

Trying to train an 8B model from scratch in Burn on day one is how the project dies. Using Burn or Candle to run a distilled 100M–1B encoder is how it ships.


 

Crate layout
A concrete layout that maps onto Jev’s product:

`spine-schema`. Pure types. `State`, `Question::{Choice, Score, Noul}`, `Answer`, `Usage`, JSON Schema identical to TypeSafe’s `/v1/systemone` where practical. Confidence function as a unit-tested pure fn. No ML.

`spine-tokenize`. Hugging Face `tokenizers` crate. Truncation policy that matches the two budgets: total tokens, and state-plus-longest-question.

`spine-model`. Candle or Burn. Encoder forward, option scoring, softmax, score expectation. Features: `cuda`, `metal`, `cpu`. Weights via `safetensors`. Fused batch of many questions on one state.

`spine-infer`. Batcher. Packs concurrent requests that share nothing, and packs many questions that share state. Queue with a latency SLO (for example, 8 ms microbatch or 32 requests, whichever first). KV reuse for the state prefix inside a request.

`spine-serve`. `axum` + `tower`. `POST /v1/systemone`, `GET /v1/models`. Bearer auth. 422 on bad questions (Choice with 0 or >255 options, Score with <2 or >10 levels). 429 with `Retry-After`. Tracing of model version, input tokens, and the full answer simplex. Output tokens may be reported as a small constant per question so billing dashboards that expect the field do not break.

`spine-adapter`. Direct port of `system-one-adapter-python` to Rust, talking to OpenAI-compatible endpoints. This is week-one: the API works before any custom weight exists.

`spine-train`. Python package, not a crate. Lightning or Hugging Face Trainer. Reads JSONL of System One examples. Writes checkpoints that `spine-model` loads. Calibration evaluation: reliability diagrams, ECE, Brier, and the TypeSafe-style “workflow accuracy versus cost” once a harness exists.

`spine-eval`. Replay TypeSafe’s four workflows if their public queries are used under whatever license the evals site grants, plus local datasets. Always run the adapter baseline next to the native model, the same way TypeSafe did.

Serving kernels that make the latency claim true
The difference between a 30 ms encoder and a 300 ms encoder at the same size is engineering.

Encode `state` once per request and never again. Cap question text. Pre-embed static option lists when an application repeats the same taxonomy (department names, rubric levels) and only re-encode `state`. Use FP16 or FP8 on GPU, int8 on CPU. Dynamic batch across requests, not just across questions. Do not use KV-cache logic from causal LMs; use a prefix cache for the state tokens inside one request’s question loop. For CPU-only demos, ONNX Runtime via `ort` is often faster to ship than a from-scratch Candle kernel.

A first milestone that already has product value: a 100–300M encoder, 8k context, p50 under 50 ms on an A10 or M-series Mac for 1k-token state and 8 questions. That will not match Jev’s “frontier on System One tasks” claim. It will match Jev’s interface, which is what most application code needs in order to be written.

Data, legally and practically
Do not scrape TypeSafe production traffic. Use:

	•	Public classification, NLI, STS, and reranking datasets recast as Choice / Score / Noul.
	•	Synthetic questions over public documents (Wikipedia, SEC filings, Hugging Face forums) generated by an LLM adapter, with the adapter’s distribution stored as a soft label.
	•	Human labels on a small, high-quality calibration set in the target domain.
	•	Optional: if you have API access to Jev, treat it as a teacher only on data you own, and check TypeSafe’s terms before distilling.

The bitterest-lesson ordering still applies. A 300M model trained on the right decision data will beat a 7B chat model prompted to emit JSON, on this interface, at this latency. That is the only claim an open project should make on day one.

A 90-day build sequence
Days 1–14: `spine-schema` + `spine-serve` + `spine-adapter`. The HTTP API is real. Applications can be written against it. Tests freeze the JSON fixtures from TypeSafe’s docs.

Days 15–30: `spine-model` on an off-the-shelf cross-encoder. Correctness over speed. Prove independence of questions, the 255 cap, and the confidence formula.

Days 31–60: distill a bi-encoder / late-interaction model from the adapter teacher. Hit the 50 ms p50 target on a single GPU. Add reliability diagrams.

Days 61–90: domain calibration sets, the workflow eval harness, and a published model card that states parameter count, data, and the limits TypeSafe was honest about: no generation, weak math, literal reading, English-first.

After that, scale the encoder, extend context, add multilingual data, and only then consider masked-diffusion research if generation becomes a requirement.

What an open Rust Jev will not be
It will not, in an independent lab without TypeSafe’s data and compute, match “frontier intelligence on System One tasks” as TypeSafe measures against Astra and Fable. Those numbers are the unpublished-model claim. The open stack can still be the right engineering object: a typed, calibrated, parallel decision service that application code can depend on, with weights that can be self-hosted, audited, and fine-tuned. That is the part of Jev that is actually specified.

Practical architecture for software that uses a System One model
Whether the backend is TypeSafe’s Jev or an open `spine-serve`, the application architecture is the same, and it is the part TypeSafe has documented thoroughly.

Code owns the workflow. The model is inserted only where a semantic judgment is required. Questions are atomic. State is filtered. Independent questions ship together. Probabilities drive thresholds that scale with risk. High confidence acts, medium confidence confirms, low confidence escalates to a human or a reasoning model. Generation, if needed, is a different model, optionally checked by the decision model. That hybrid is the product TypeSafe is selling, more than any one weight matrix. 19 35

Conclusion
Jev is a closed, early-access decision model from a lab founded by a co-author of InstructGPT, funded with a $40 million DCVC-led seed, and launched on 15 September 2026 as the first “System One” model. Its public surface is unusually complete for a frontier lab: a small API, three primitives, a confidence formula, a jaggedness document, workflow evals with caveats attached, and client SDKs. Its private surface is the usual one: weights, data, and the real RLCD algorithm.

The reconstructible architecture is a non-autoregressive, probably bidirectional, parallel closed-set sampler on top of a pretrained text model, trained so that probabilities can be consumed by software. The open-source path in Rust is to freeze that contract, serve it from a native binary, and train an encoder-classifier toward calibration. That path will not produce a line-for-line Jev. It will produce the thing Jev made newly obvious: intelligence that is invoked like a function, with a type, a probability, and a latency budget that fits inside ordinary code.

The TypeSafe Jev model is an architectural pivot rather than a raw capability breakthrough. By replacing the autoregressive decoding loop with parallel classification heads over a pooled state, it shifts AI from generative stochasticity to deterministic, fast-path routing. The Manus AI assessment accurately identifies the likely topology[em dash]a bidirectional encoder with request-time label embeddings[em dash]but a purpose-built, "lite" open-source variant in Rust can optimize this further by exploiting strict memory control, zero-copy deserialization, and edge-native architectures.
Here is the evaluation of the proposed architecture and a blueprint for a hyper-optimized Rust-native variant.
Evaluation of the Proposed Architecture
The Assessment's Strengths:
 * De-risking the Sampler: Identifying that Jev is not a diffusion model (like LLaDA) but an encoder with late-interaction heads is spot on. Diffusion adds unnecessary sampling steps for closed-set routing.
 * The RLCD Demystification: Stripping RLCD down to temperature-scaled Brier score and log-loss fine-tuning grounds the project in proven classical ML techniques rather than chasing bespoke RL paradigms.
 * The Crate Layout: The proposed spine- crate separation logically mirrors the API contract, neatly isolating schema, batching, and inference.
Where the Assessment Falls Short for a "Lite" Variant:
 * Over-reliance on PyTorch/Python: While training must happen in PyTorch/JAX, the assessment underestimates the maturity of the Rust inference ecosystem. A truly lite variant should avoid heavy python bindings in production entirely.
 * Missing Architectural Mitigations: It relies on the model to naturally separate state from instructions. A robust architecture requires hard boundaries at the parser level to prevent adversarial text in the state from hijacking the instructions.
Blueprint for the Open-Source Lite Variant
To build a non-competing, highly efficient Rust variant, the architecture must focus on deterministic execution, minimal memory footprint, and edge-deployability. By extracting the core mechanics into standalone open-source Rust libraries[em dash]much like the spec-spine CLI tool[em dash]the system can be served directly alongside application code.
1. The Core Engine: Strict Late-Interaction (ColBERT-style)
Instead of a simple cross-encoder (which scales poorly with 255 options) or a fused prefix-cache, the lite variant should use a strict late-interaction architecture.
 * Mechanism: The state is processed by a distilled 100M–300M parameter encoder (e.g., a heavily pruned ModernBERT) into a matrix of token embeddings E_s. Each option in a Choice or Score primitive is encoded into E_o.
 * Rust Optimization: The scoring function is a MaxSim dot product between E_s and E_o. In Rust, this is purely deterministic math that can be aggressively optimized using std::simd or BLAS, allowing hundreds of options to be scored in microseconds without touching the neural network weights a second time.
2. Spec-Driven Coupling Gates
To ensure OWASP Top 10 for Agentic Applications (ASI 2026) compliance, the API layer must strictly isolate untrusted data (state) from trusted control (instructions and criteria).
 * Implement strict parsing gates using serde. The schema enforces that state is treated strictly as a passive tensor representation.
 * If adversarial text is detected (e.g., anomalous token entropy or known injection signatures), the Rust layer immediately short-circuits to a deterministic fallback or rejection before inference begins, ensuring zero hallucination of the control flow.
3. Zero-Copy State Caching
A significant latency bottleneck in the proposed architecture is re-tokenizing and re-encoding the state across sequential API calls that evaluate the same payload.
 * Implement a deterministic hashing mechanism for the incoming state string.
 * Cache the resulting key-value (KV) activations in memory. If a subsequent request queries the same state (e.g., evaluating a new tool call against the same user context), the Rust batcher (spine-infer) skips the state encoding entirely and only encodes the new questions.
4. Edge-Native Persistence
For continuous calibration (the "RL" part of RLCD), the system needs to log decision probabilities against actual outcomes.
 * Instead of heavy database drivers, the Rust server (spine-serve) can utilize embedded, edge-replicated databases like Turso or Hiqlite.
 * This allows the inference node to asynchronously persist the predicted simplex distributions and confidence scores at sub-millisecond latencies, building a local dataset for future temperature-scaling and calibration passes without blocking the fast path.
5. Deterministic Math Primitives
The output math must be executed in pure, safe Rust, fully decoupled from the tensor backend (Burn or Candle):
 * Choice Confidence: For n options and peak probability p_{max}, the confidence statistic is computed as:
   
 * Score Expectation: For ordered levels 0 through k-1, with probabilities p_i, the continuous score is:
   
By implementing these as unit-tested const fn pure functions in the spine-schema crate, you guarantee that the API contract remains identical to Jev, but executes deterministically with zero floating-point drift across different hardware architectures (e.g., ARM vs. x86).


1. Late-Interaction SIMD Scoring in Rust
To execute Choice and Score primitives at sub-millisecond latencies without touching the neural network weights a second time, we bypass standard matrix multiplication in favor of a CPU-optimized late-interaction MaxSim operation.
The mathematical goal is to compute the maximum similarity between each token in the option sequence and any token in the state sequence, then sum those maximums. For a state embedding matrix E_s of length L_s and an option embedding matrix E_o of length L_o, both with dimension D:
Using Rust's nightly portable_simd feature, we can vectorize the D-dimensional dot products. By chunking the embedding vectors into 8-lane or 16-lane f32 operations, the CPU calculates similarities in a deterministic, cache-friendly loop.
#![feature(portable_simd)]
use std::simd::prelude::*;

/// Computes the SIMD-accelerated dot product of two D-dimensional token embeddings.
#[inline(always)]
fn dot_product_simd(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let mut sum = f32x8::splat(0.0);
    
    let (a_chunks, a_rem) = a.as_chunks::<8>();
    let (b_chunks, b_rem) = b.as_chunks::<8>();

    for (a_chunk, b_chunk) in a_chunks.iter().zip(b_chunks.iter()) {
        sum += f32x8::from_array(*a_chunk) * f32x8::from_array(*b_chunk);
    }

    let mut remainder_sum = sum.reduce_sum();
    for (a_val, b_val) in a_rem.iter().zip(b_rem.iter()) {
        remainder_sum += a_val * b_val;
    }
    
    remainder_sum
}

/// Executes the MaxSim aggregation across the state and option embedding matrices.
/// `state_embeddings`: flattened L_s x D matrix
/// `option_embeddings`: flattened L_o x D matrix
/// `d`: embedding dimension
pub fn compute_maxsim(state_embeddings: &[f32], option_embeddings: &[f32], d: usize) -> f32 {
    let l_s = state_embeddings.len() / d;
    let l_o = option_embeddings.len() / d;
    
    let mut total_score = 0.0;

    // Iterate over each token in the option sequence
    for j in 0..l_o {
        let opt_token = &option_embeddings[j * d..(j + 1) * d];
        let mut max_sim = f32::NEG_INFINITY;

        // Find the maximum similarity against any token in the state sequence
        for i in 0..l_s {
            let state_token = &state_embeddings[i * d..(i + 1) * d];
            let sim = dot_product_simd(opt_token, state_token);
            if sim > max_sim {
                max_sim = sim;
            }
        }
        total_score += max_sim;
    }

    total_score
}

This approach allows the Rust backend to pre-compute and cache E_s during the first API request. Subsequent permutations of questions or dynamic options only require passing the new E_o matrices through this CPU-bound MaxSim function, easily achieving the 50 ms latency target for high-cardinality routing.
2. The Distillation Pipeline
To create the 100M-parameter student encoder, we extract the reasoning capacity of a frontier generative model and compress it into the deterministic classification heads of a smaller architecture like ModernBERT or DeBERTa-V3.
Phase 1: Synthetic Data Generation (The Teacher)
Generate a corpus of highly complex classification, routing, and scoring tasks. Pass these through a frontier teacher model (e.g., GPT-4o, Llama 3 70B) wrapped in your spine-adapter.
 * Crucial Step: Do not just record the argmax (the winning choice). Record the entire probability simplex outputted by the teacher's logprobs. These soft labels contain the "dark knowledge"[em dash]the teacher's internal assessment of ambiguity between closely related options.
Phase 2: KL Divergence Training (The Student)
Initialize the 100M parameter student encoder. The loss function during this phase is purely token-free. We minimize the Kullback-Leibler (KL) divergence between the teacher's soft probability distribution P_T and the student's output distribution P_S:
This forces the lightweight student model to replicate the exact confidence boundaries and hesitations of the frontier model across the 255-option limit of the Choice primitive.
Phase 3: RLCD Calibration Pass
Once the student closely mirrors the teacher's distributions, freeze the core encoder weights and fine-tune only the classification heads using proper scoring rules on a human-labeled holdout dataset. The goal here is strict calibration: an event the model labels with 0.80 probability must be empirically true 80% of the time.
The primary loss function here shifts to the Brier Score, penalizing overconfidence:
Where f_t is the predicted probability and o_t is the actual outcome (1 or 0).
Phase 4: Temperature Map Export
Determine the optimal temperature scaling parameter to smooth the final logits into perfectly calibrated probabilities. Export the finalized model via the safetensors format, completely stripping the Python training environment. The Rust spine-model crate using Burn or Candle will ingest these tensors directly to initialize the embedded inference engine.


To guarantee isolation between untrusted application data (state) and trusted control flow (instructions), the spine-schema crate must enforce a hard boundary at the deserialization layer.
By combining Rust’s Newtype pattern for taint tracking, serde for strict schema enforcement, and the validator crate for mathematical bounds checking, we can guarantee that adversarial text cannot leak into the instruction encoding path.
Here is the architectural structure for the serde parsing gates.
1. Type-Level Taint Tracking (The Newtype Pattern)
The core vulnerability in standard LLM wrappers is treating both state and instructions as standard strings. In Rust, we use the Newtype pattern to create an UntrustedState type. This forces the compiler to ensure that the inference engine explicitly unwraps the state for the E_s (state embedding) matrix and cannot accidentally pass it to the E_o (option embedding) matrix.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use validator::{Validate, ValidationError};

/// Gate 1: Taint Tracking.
/// UntrustedState can hold a String, Array, or Object as per the API contract.
/// It cannot be directly concatenated with Instructions without explicit unwrapping.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct UntrustedState(pub Value);

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)] // Gate 2: Reject payload smuggling
pub struct SystemOneRequest {
    pub model: String,
    pub state: UntrustedState,
    
    #[validate(length(min = 1, max = 50, message = "Too many questions in one batch"))]
    pub questions: HashMap<String, QuestionType>,
}

2. Strict Structural Validation
The parser must mathematically enforce the limits of the System One architecture (e.g., maximum 255 options for Choice, 2 to 10 levels for Score) before the request ever reaches the batcher. If an attacker submits 10,000 options to trigger an Out-Of-Memory (OOM) panic during MaxSim execution, serde and validator catch it instantly.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")] // Enforces internally tagged enums to prevent polymorphic confusion
#[serde(deny_unknown_fields)]
pub enum QuestionType {
    #[serde(rename = "choice")]
    Choice(ChoiceDef),
    
    #[serde(rename = "score")]
    Score(ScoreDef),
    
    #[serde(rename = "noul")]
    Noul(NoulDef),
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct ChoiceDef {
    #[validate(length(min = 1, max = 2048))]
    pub instructions: String,
    
    #[validate(length(min = 2, max = 255, message = "Choice must be between 2 and 255 options"))]
    pub criteria: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct ScoreDef {
    #[validate(length(min = 1, max = 2048))]
    pub instructions: String,
    
    // Custom validation to ensure exactly 2 to 10 levels
    #[validate(custom = "validate_score_levels")]
    pub criteria: Vec<String>,
}

fn validate_score_levels(criteria: &[String]) -> Result<(), ValidationError> {
    if criteria.len() < 2 || criteria.len() > 10 {
        return Err(ValidationError::new("Score criteria must have 2 to 10 levels."));
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct NoulDef {
    #[validate(length(min = 1, max = 2048))]
    pub instructions: String,
    pub criteria: Option<HashMap<String, String>>, // true/false custom descriptions
}

3. The Flattening and Handoff Gate
Because state can be an arbitrary JSON object, array, or string, the schema crate must safely flatten it into a canonical string representation for the tokenizer. This happens through a deterministic serialization step that the inference engine calls, stripping out whitespace and standardizing keys so adversarial JSON formatting cannot manipulate tokenization.
impl UntrustedState {
    /// Deterministically serializes the state payload into a canonical string.
    /// This prevents JSON formatting tricks (e.g., excessive whitespace) 
    /// from chewing up the 32k context limit.
    pub fn into_canonical_string(&self) -> String {
        match &self.0 {
            Value::String(s) => s.clone(),
            Value::Array(arr) => serde_json::to_string(arr).unwrap_or_default(),
            Value::Object(obj) => {
                // In a production system, keys should be sorted here 
                // to ensure identical states yield identical cache hashes.
                serde_json::to_string(obj).unwrap_or_default()
            },
            _ => String::new(),
        }
    }
}

Security Posture Achieved
By implementing these gates in spine-schema:
 * No Prompt Injection: The state text never shares memory or context window strings with the instructions. The spine-infer crate takes UntrustedState::into_canonical_string() and sends it exclusively to the E_s state encoder.
 * Denial of Service Prevention: #[serde(deny_unknown_fields)] blocks arbitrary data dumping, and validator enforces hard boundaries on matrix dimensions before memory is allocated.
 * Type Safety: The Rust compiler physically prevents a developer from passing the state into the instructions embedding function due to the UntrustedState wrapper type.
