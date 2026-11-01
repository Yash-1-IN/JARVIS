# JARVIS --- Project Brief & Engineering Specification

**Document purpose:** Source brief for an architecture/design refinement
pass, followed by implementation in Claude Code.\
**Project status:** Pre-development / Milestone 0\
**Working name:** JARVIS\
**Initial repository name:** `jarvis`\
**Application identifier:** `com.jarvis.desktop`

------------------------------------------------------------------------

## 1. Executive Summary

Build **JARVIS**, a cross-platform, voice-first personal desktop AI
assistant inspired by the sophistication and utility of JARVIS from
*Iron Man*.

JARVIS should feel like an intelligent operating layer for a personal
computer, not a chatbot trapped in a window. It should eventually be
able to understand spoken requests, reason about tasks, remember useful
context, observe relevant system information, and operate approved
applications and files.

The product should combine: - A polished, cinematic but usable desktop
interface. - Natural, low-latency voice interaction with a custom wake
phrase. - A replaceable AI-provider layer. - Persistent, user-controlled
memory. - A secure tool system for interacting with the operating
system. - Local processing wherever practical, with cloud intelligence
when useful.

The project is ambitious. It must be developed incrementally, with a
working, tested foundation before advanced features are attempted.

## 2. Product Vision

### 2.1 Experience goals

JARVIS should: 1. Feel immediately available without requiring the user
to open a chat and type. 2. Respond naturally through speech and text.
3. Be interruptible while speaking or performing a task. 4. Understand
enough context to avoid making the user repeat themselves. 5. Explain
what it intends to do before consequential actions. 6. Ask for approval
when an action could have meaningful consequences. 7. Remain useful even
when cloud AI services are unavailable. 8. Be transparent about
microphone, network, memory, and action status.

### 2.2 Product personality

-   Voice: refined, composed British male voice.
-   Communication: concise by default; detailed when asked.
-   Personality: capable, calm, subtly witty when appropriate; never
    theatrical to the point of slowing down useful work.
-   Visual identity: sophisticated blue/cyan holographic interface
    inspired by the Iron Man JARVIS aesthetic.
-   The interface must remain legible and functional. Visual effects
    must not compromise usability or accessibility.

### 2.3 Target platforms

Support from the beginning: - Windows - macOS - Linux

Prioritise a reliable common core and isolate operating-system-specific
functionality behind platform adapters.

### 2.4 Target hardware

Optimise for an average modern laptop, not only high-end machines.

Design constraints: - Avoid requiring a dedicated GPU. - Keep idle CPU
and memory usage low. - Prefer lightweight local models and services
where feasible. - Make expensive visual effects optional or scalable. -
Do not assume a discrete graphics card or large amount of RAM.

## 3. Core Product Requirements

### 3.1 Voice interaction

The intended interaction is always-listening wake-word detection using:

**Wake phrase:** "Hey JARVIS"

Expected flow: 1. A local wake-word detector monitors microphone input.
2. Before activation, audio should remain local and should not be sent
to a cloud service. 3. On wake-word detection, JARVIS indicates that it
is active and begins listening for the request. 4. Voice activity
detection determines when the user has finished speaking. 5.
Speech-to-text produces a transcript. 6. The assistant reasons and, if
necessary, requests tools or approval. 7. Text-to-speech speaks the
response. 8. The user can interrupt at any time.

Requirements: - A visible microphone state and obvious mute control. - A
clear distinction between muted, standby, listening, processing,
speaking, interrupted, and error states. - Support text input as a
fallback. - Handle false wake activations and background noise
gracefully. - Support cancellation and interruption throughout the
pipeline. - Do not store raw microphone recordings by default. - Do not
send pre-wake audio to cloud services.

**Important technical uncertainty:** A wake-word engine does not
automatically provide a reliable custom "Hey JARVIS" model. Evaluate
available engines and keyword models with real-world tests before
committing to one.

### 3.2 Speech recognition and synthesis

Evaluate local options before selecting production dependencies.

Potential technologies to investigate: - Wake word: `sherpa-onnx` or
another actively maintained local engine. - Voice activity detection:
Silero VAD or a suitable alternative. - Speech-to-text: `whisper.cpp` or
another suitable local implementation. - Text-to-speech: Piper or
another suitable local implementation.

Selection criteria: - Cross-platform support. - Latency on average
laptop hardware. - CPU and memory requirements. - Naturalness and
intelligibility. - Model size and download experience. - Commercial and
redistribution licensing. - Ease of packaging and updating.

Do not bundle a voice or model asset until its licence and
redistribution terms have been checked. Audition candidate voices; do
not assume that a technically compatible voice will sound refined or
convincingly British.

### 3.3 AI brain

The first cloud provider will likely be Anthropic, but the application
must not be tightly coupled to one provider.

Implement a provider abstraction that supports: - Sending conversation
context. - Streaming responses. - Tool/function requests. - Cancellation
and timeouts. - Provider errors and rate limits. - Model selection
through configuration.

The AI provider must be replaceable without rewriting the conversation
system, tools, memory, or UI.

**Security boundary:** The model is not trusted code. It may propose
actions, but it must never directly execute arbitrary shell commands or
receive unrestricted filesystem access.

### 3.4 Memory

Memory should be local-first, persistent, searchable, and
user-controlled.

Planned capabilities: - Store conversation history. - Store explicitly
approved user memories. - Retrieve relevant memories for a
conversation. - Allow users to inspect, edit, and delete memories. -
Distinguish durable memories from temporary conversation context. -
Avoid silently treating model-generated guesses as facts.

SQLite is the likely initial database choice.

Memory must be designed as a distinct subsystem, not scattered
throughout frontend state or prompt strings.

### 3.5 Tools and computer interaction

JARVIS should eventually be able to: - Open approved applications. -
Search user-approved directories. - Find and organise files within
defined boundaries. - Read selected files when the user grants access. -
Interact with supported websites and applications. - Retrieve basic
system information. - Perform other useful tasks through explicitly
registered tools.

All actions must go through a **tool registry** and a separate
**permission gateway**.

Each tool should define: - Stable tool name and description. - Strict
input schema and validation. - Permission/risk classification. -
Resource boundaries (allowed folders, applications, domains, etc.). -
Timeout and cancellation behaviour. - Result schema. - Audit-log
behaviour.

Never expose an unrestricted shell or filesystem tool to the model.

### 3.6 Permission model

Use risk-based permissions.

  -----------------------------------------------------------------------
  Risk level              Examples                Default behaviour
  ----------------------- ----------------------- -----------------------
  Low                     Read basic system       May run automatically
                          status; search an       within its defined
                          explicitly allowed      scope
                          folder                  

  Medium                  Move or rename files;   Ask for confirmation or
                          modify a user-selected  follow a
                          document                user-configured policy

  High                    Delete files; execute   Require explicit
                          scripts; install        approval immediately
                          software; send          before execution
                          messages; make          
                          purchases; change       
                          security settings       

  Prohibited              Actions that violate    Never execute
                          safety policy or exceed 
                          the application's       
                          legitimate authority    
  -----------------------------------------------------------------------

Additional requirements: - Approval must identify the exact action and
relevant target. - Do not treat broad consent as approval for unrelated
future actions. - Do not let model instructions override the permission
gateway. - Provide a visible activity/audit history. - Support
cancellation. - Apply timeouts and resource limits. - Restrict file
access to explicitly allowed paths. - Use minimal Tauri capabilities and
permissions.

### 3.7 System awareness

Eventually show useful, non-invasive system information: - CPU usage. -
Memory usage. - Battery level and charging state, where available. -
Relevant application/window context, only with appropriate permission. -
Network/service status where useful.

System telemetry must be lightweight and must not become a constant
resource drain.

## 4. Interface and Visual Direction

### 4.1 Overall aesthetic

Create a sophisticated blue holographic interface: - Deep navy /
near-black foundation. - Electric blue and cyan highlights. - Subtle
translucent glass surfaces. - Fine technical lines, restrained glow, and
layered depth. - Refined typography. - Smooth, purposeful transitions. -
A central circular holographic visualisation.

Avoid: - Excessive neon. - Too many competing panels. - Decorative
information that looks important but has no function. - Generic
AI-dashboard layouts. - Constant animation that distracts or consumes
resources.

### 4.2 Main interface areas

Initial interface should include: 1. **Central hologram:** primary
JARVIS identity and live state visualisation. 2. **Conversation panel:**
user and assistant messages, with streaming response support. 3.
**Audio/status control:** microphone state, mute, listening, processing,
and speaking indicators. 4. **Activity panel:** recent tool activity and
pending approvals. 5. **Settings:** provider configuration, voice,
privacy, permissions, and appearance.

The first milestone may use mock interaction states, but all mock
behaviour must be clearly separated from production functionality.

### 4.3 Interaction and accessibility

-   Text must remain readable at normal desktop sizes.
-   Provide keyboard navigation and visible focus states.
-   Respect reduced-motion preferences.
-   Make mute and cancel controls easy to reach.
-   Do not rely on colour alone to communicate system state.
-   Provide a usable interface when 3D rendering is unavailable or
    disabled.

## 5. Architecture

### 5.1 Recommended stack

  -----------------------------------------------------------------------
  Layer                   Initial choice          Rationale
  ----------------------- ----------------------- -----------------------
  Desktop framework       Tauri 2                 Lightweight
                                                  cross-platform desktop
                                                  shell

  Frontend                React + TypeScript +    Familiar, typed,
                          Vite                    productive UI stack

  3D interface            Three.js + React Three  Holographic
                          Fiber + Drei            visualisation

  Frontend state          Zustand                 Small, straightforward
                                                  state management

  Frontend validation     Zod                     Runtime validation for
                                                  frontend data

  Backend                 Rust                    System integration,
                                                  performance, safety
                                                  boundaries

  Async runtime           Tokio, when needed      Audio and background
                                                  task orchestration

  Local database          SQLite                  Local-first persistent
                                                  storage

  AI provider             Anthropic initially,    Initial reasoning
                          behind an abstraction   provider

  Browser automation      Playwright, when needed Controlled browser
                                                  workflows

  Logging                 `tracing` +             Structured Rust logging
                          `tracing-subscriber`    

  Frontend tests          Vitest + React Testing  UI and logic tests
                          Library                 

  Backend tests           Rust unit/integration   Core behaviour and
                          tests                   safety tests
  -----------------------------------------------------------------------

Keep dependencies minimal. Do not install every anticipated technology
during the initial bootstrap.

### 5.2 Architectural principles

1.  **Local core, replaceable intelligence:** wake-word detection, app
    state, memory storage, permissions, tools, and basic system
    functions must not depend on a cloud model.
2.  **Clear boundaries:** UI, audio, intelligence, memory, tools,
    permissions, and platform integration are separate modules.
3.  **Typed contracts:** frontend/backend communication uses explicit,
    versionable event and command types.
4.  **Least privilege:** grant only the permissions required for a
    specific feature.
5.  **Observable behaviour:** important operations produce structured
    logs and user-visible activity.
6.  **Cancellation by design:** long-running operations must be
    cancellable.
7.  **Cross-platform adapters:** isolate platform-specific APIs.
8.  **Incremental delivery:** each milestone must build and run before
    the next is added.

### 5.3 High-level data flow

``` text
Microphone
   |
Local wake-word detector
   |
Voice activity detection
   |
Speech-to-text
   |
Conversation / context manager
   |
AI provider abstraction
   |
Tool request? ---------------------- No ------------------> Response
   |
Tool registry
   |
Input validation
   |
Permission gateway
   |
User approval, if required
   |
Platform adapter / controlled tool
   |
Audited result
   |
Conversation response
   |
Text-to-speech
   |
Speaker
```

The model proposes tool calls. The backend validates and authorises
them. The model must never bypass this path.

### 5.4 Initial repository structure

Start with one Tauri application and modular folders. Do not split the
project into many Rust crates prematurely.

``` text
jarvis/
├── src/
│   ├── components/
│   │   ├── hologram/
│   │   ├── conversation/
│   │   ├── audio/
│   │   ├── dashboard/
│   │   └── common/
│   ├── features/
│   │   ├── assistant/
│   │   ├── settings/
│   │   └── activity/
│   ├── hooks/
│   ├── stores/
│   ├── services/
│   ├── types/
│   ├── styles/
│   ├── App.tsx
│   └── main.tsx
├── src-tauri/
│   ├── src/
│   │   ├── audio/
│   │   ├── intelligence/
│   │   ├── memory/
│   │   ├── permissions/
│   │   ├── tools/
│   │   ├── system/
│   │   ├── events/
│   │   ├── lib.rs
│   │   └── main.rs
│   ├── capabilities/
│   └── tauri.conf.json
├── docs/
├── tests/
├── CLAUDE.md
├── README.md
└── package.json
```

Keep the generated Tauri bootstrap files intact and add modules
incrementally.

## 6. Shared Frontend/Backend Contracts

The following is an initial contract sketch, not a final immutable API.
Refine it before implementation and document any changes.

``` typescript
export type JarvisState =
  | "idle"
  | "activated"
  | "listening"
  | "processing"
  | "speaking"
  | "interrupted"
  | "muted"
  | "error";

export interface JarvisEvent<T = unknown> {
  id: string;
  timestamp: number;
  type: string;
  payload: T;
}

export interface ConversationMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: number;
}

export interface SystemStatus {
  cpuUsage: number | null;
  memoryUsage: number | null;
  batteryLevel: number | null;
  isCharging: boolean | null;
}

export interface PendingAction {
  id: string;
  description: string;
  toolName: string;
  riskLevel: "low" | "medium" | "high";
  requiresApproval: boolean;
}
```

Refinement should decide: - Event naming and versioning. -
Command/request-response conventions. - Error representation. -
Streaming event format. - Whether timestamps use Unix milliseconds or
another standard. - How cancellation IDs are represented. - How tool
approval requests are represented. - How Rust and TypeScript types
remain consistent.

The frontend must reflect backend state rather than inventing a second,
conflicting state machine.

## 7. Milestone Plan

The timeline is indicative, not a deadline. Prefer a tested milestone
over a rushed feature.

### M0 --- Foundation

**Goal:** A clean, reproducible, cross-platform desktop application.

Deliverables: - Tauri 2 + React + TypeScript project bootstrapped. -
Repository and branch strategy established. - Rust module structure
created. - Basic Tauri ping command registered and callable. -
Structured logging configured. - Initial shared contracts documented. -
Basic backend test. - Formatting and type-check scripts. - Build/dev
instructions for Windows, macOS, and Linux. - CI checks where practical.

Explicitly out of scope: - AI provider integration. - Wake-word/audio
pipeline. - Persistent memory. - Filesystem automation. - Shell
execution.

### M1 --- Interface

**Goal:** A polished, interactive desktop shell using mock events.

Deliverables: - Design tokens and typography. - Main application
layout. - Central hologram. - Conversation panel. - Microphone and
assistant status indicators. - Activity panel and approval-card
design. - Settings shell. - Responsive desktop layouts. - Reduced-motion
and fallback rendering. - Mock state machine clearly labelled as mock.

### M2 --- Voice

**Goal:** A working local voice pipeline.

Deliverables: - Microphone capture. - Local wake-word detection. -
VAD. - Speech-to-text. - Text-to-speech. - Mute and microphone state. -
Interruption and cancellation. - Error handling and device selection. -
Measured latency and CPU/memory use.

Do not claim "Hey JARVIS" works until it has been tested in realistic
conditions.

### M3 --- Brain

**Goal:** Natural conversations with a cloud model.

Deliverables: - AI provider interface. - Anthropic implementation. -
Secure API-key storage. - Streaming responses. - Conversation context
manager. - Timeouts, cancellation, retries where appropriate. -
Rate-limit and network error handling. - Text chat before full voice
integration.

### M4 --- Memory

**Goal:** Persistent, user-controlled context.

Deliverables: - SQLite schema and migrations. - Conversation
persistence. - Explicit memory creation. - Memory retrieval. - Memory
inspection, editing, and deletion. - Clear separation of conversation
history and durable memory. - Tests for persistence and deletion.

### M5 --- Hands

**Goal:** Safe, useful computer actions.

Deliverables: - Tool registry. - Strict input validation. - Permission
gateway. - Approval UI. - Audit log. - Basic system-status tools. -
Approved application launching. - Restricted file search. - Carefully
scoped file organisation tools. - Cancellation, timeouts, and failure
recovery. - Security tests.

### M6 --- Integration and Release

**Goal:** A stable, packaged cross-platform alpha.

Deliverables: - Windows/macOS/Linux build verification. -
Installer/package workflow. - Startup and shutdown reliability. -
Audio-device recovery. - Crash/error recovery. - Resource and latency
profiling. - Security review. - User-facing setup and privacy
documentation. - Known limitations documented.

## 8. Development Workflow and Team Split

Two developers are working together.

### Developer A --- Backend

Owns: - Rust core. - Audio pipeline. - Wake-word, VAD, STT, and TTS
integration. - AI provider abstraction. - Context management. - Memory
and database. - Tool registry. - Permission gateway. - Platform
integrations.

### Developer B --- Frontend

Owns: - React and TypeScript. - Design system. - Three.js hologram. -
Audio visualisation. - Conversation interface. - Settings. - Activity
and approval UI. - Frontend tests.

### Shared responsibilities

Both developers own: - Architecture decisions. - Shared contracts. -
Integration tests. - Security review. - Cross-platform verification. -
Release decisions.

Workflow: - Use feature branches and pull requests into `develop`. -
Merge tested releases into `main`. - Agree on shared contracts before
parallel implementation. - Avoid editing the other developer's module
boundaries without coordination. - Git worktrees may be used for
parallel Claude Code sessions.

## 9. Security, Privacy, and Reliability Requirements

These requirements are foundational and must not be deferred until the
end.

### Privacy

-   Wake-word detection is local.
-   No pre-wake audio is sent to cloud services.
-   Raw microphone recordings are not retained by default.
-   Clearly show microphone and mute state.
-   Explain when a request or context is sent to a cloud provider.
-   Keep persistent memory inspectable and editable.
-   Store secrets using the operating system's secure credential store
    where possible.

### Security

-   Never put API keys in frontend code or `VITE_` environment
    variables.
-   Never give the model unrestricted shell access.
-   Never give the model unrestricted filesystem access.
-   Validate every tool input in the backend.
-   Enforce permissions outside the model.
-   Restrict filesystem paths and supported domains.
-   Use minimal Tauri capabilities.
-   Treat external content, files, webpages, and model output as
    untrusted.
-   Log consequential actions without unnecessarily logging private
    content.
-   Require explicit approval for high-risk actions.

### Reliability

-   Every long-running task needs timeout and cancellation behaviour.
-   Audio failures must not crash the application.
-   Device disconnection and reconnection must be handled.
-   Cloud failures must produce understandable errors.
-   The app should remain usable without the AI provider.
-   Avoid blocking the UI thread.
-   Test on real hardware, not only mocks.

## 10. Initial Setup and Bootstrap

Use the official Tauri 2 project generator. Confirm current
prerequisites against the official Tauri documentation before setup.

Recommended first-machine sequence:

1.  Create an empty private GitHub repository named `jarvis`. Do not
    initialise it with a README or licence.
2.  From the parent directory, run:

``` bash
pnpm create tauri-app
```

3.  Select:
    -   Project name: `jarvis`
    -   Identifier: `com.jarvis.desktop`
    -   Language: TypeScript / JavaScript
    -   Package manager: pnpm
    -   UI template: React
    -   UI flavour: TypeScript
4.  Enter the generated directory and initialise Git:

``` bash
cd jarvis
git init -b main
git add .
git commit -m "chore: bootstrap JARVIS desktop app"
git remote add origin YOUR_GITHUB_REPOSITORY_URL
git push -u origin main
```

5.  On the second machine:

``` bash
git clone YOUR_GITHUB_REPOSITORY_URL
cd jarvis
pnpm install
pnpm tauri dev
```

Do not clone an empty repository and then run the generator into it if
that would create a nested project directory. Generate first, then
connect the generated project to the remote.

### Initial frontend dependencies

Inspect the generated `package.json` first. The Tauri template already
includes the core React/Vite/Tauri dependencies.

Add only the planned UI and test dependencies:

``` bash
pnpm add three @react-three/fiber @react-three/drei
pnpm add zustand zod lucide-react
pnpm add -D vitest jsdom
pnpm add -D @testing-library/react
pnpm add -D @testing-library/jest-dom
pnpm add -D prettier
```

### Initial Rust dependencies

Inspect `src-tauri/Cargo.toml` first. Keep the generated Tauri
dependencies.

``` bash
cd src-tauri
cargo add tracing
cargo add tracing-subscriber --features env-filter,fmt
cd ..
```

### Initial verification

``` bash
pnpm install
pnpm tauri info
pnpm tauri dev
```

Then run the project's configured formatting, type-check, test, and Rust
check commands. Add scripts only after inspecting the generated
template.

Do not add SQLite, audio engines, wake-word models, Anthropic SDKs, or
Playwright at bootstrap. Add them when their milestone begins and their
requirements are understood.

## 11. Claude Code Project Instructions

The repository root must contain a `CLAUDE.md` file. It should instruct
Claude Code to:

-   Read the project brief and inspect the repository before changing
    code.
-   Plan before implementation.
-   Work in small, composable modules.
-   Respect frontend/backend ownership boundaries.
-   Coordinate before changing shared contracts.
-   Keep generated Tauri bootstrap files intact unless a change is
    necessary.
-   Avoid unnecessary dependencies.
-   Use typed events and validated payloads.
-   Include error handling, logging, and tests.
-   Keep platform-specific code behind adapters.
-   Never expose unrestricted shell or filesystem access to the model.
-   Enforce approvals in backend code, not prompt instructions.
-   Keep API keys out of source and frontend bundles.
-   Keep pre-wake audio local.
-   Implement cancellation and timeouts.
-   Never present mock functionality as production functionality.
-   Run relevant tests, formatting, linting, and type checks before
    reporting completion.
-   Report changed files, tests run, limitations, and remaining risks.
-   Do not attempt to build every milestone in one pass.

### Developer A --- initial M0 prompt

``` text
We are implementing JARVIS Milestone 0.

Read CLAUDE.md and inspect the entire repository before making changes.
You own the Rust backend.

Tasks:
- Establish the initial Rust module structure.
- Preserve the generated Tauri bootstrap.
- Add a basic ping command and register it with Tauri.
- Configure structured logging.
- Define/document the initial backend event types and frontend contract.
- Add a basic Rust unit test.
- Document the backend API and how the frontend invokes it.

Do not implement AI, audio, filesystem access, or shell execution yet.
Do not modify frontend files.
Do not add unnecessary dependencies.

First provide a short implementation plan. Then implement, run cargo check and tests, and report changed files, test results, and limitations.
```

### Developer B --- initial M0/M1 prompt

``` text
We are implementing JARVIS Milestone 0 and the interface foundation of Milestone 1.

Read CLAUDE.md and inspect the repository before making changes.
You own the React frontend.

Tasks:
- Create design tokens and typography.
- Build the desktop application shell.
- Build the central hologram component.
- Build the conversation interface.
- Build microphone and assistant status indicators.
- Build a mock assistant state machine and mock events.
- Add responsive desktop layouts and reduced-motion support.

Visual direction:
Deep navy, electric blue and cyan, a central circular hologram, subtle glass surfaces, fine technical details, refined typography, and restrained animation.

Do not modify Rust files.
Do not claim to implement real voice recognition; all mock behaviour must be clearly identified.
Keep the interface legible and usable rather than making every element glow.

First provide a short implementation plan. Then implement, run frontend tests and TypeScript checks, and report changed files, test results, and limitations.
```

## 12. Initial Acceptance Criteria

### M0 is complete when:

-   [ ] The app starts in development mode on both developers' machines.
-   [ ] The project builds using the documented setup.
-   [ ] Rust modules are organised without breaking Tauri bootstrap.
-   [ ] The frontend can invoke the ping command.
-   [ ] Structured logging works.
-   [ ] Shared contracts are documented.
-   [ ] Basic Rust tests pass.
-   [ ] TypeScript checks and frontend tests pass.
-   [ ] Both developers can pull, install, run, and contribute.

### M1 is complete when:

-   [ ] The primary interface is implemented.
-   [ ] The hologram responds to mock assistant states.
-   [ ] Conversation messages render correctly.
-   [ ] Audio and assistant states are visually distinguishable.
-   [ ] The activity/approval area is present.
-   [ ] The UI remains usable at common desktop window sizes.
-   [ ] Reduced-motion and non-3D fallback behaviour work.
-   [ ] No mock feature is misrepresented as functional.

### General definition of done

A feature is not complete merely because it appears to work once. It
must have: - A clear owner and scope. - Typed interfaces. - Error
handling. - Cancellation where applicable. - Tests appropriate to its
risk. - Documentation for setup and behaviour. - No known critical
security issue. - A clear distinction between implemented, mocked, and
planned behaviour.

## 13. Budget and Cost Discipline

The team has approximately **\$200 in Claude Code usage allowance** for
development. This is a development allowance, not a guarantee of how
much work can be completed.

Suggested planning allocation:

  Area                            Planning allowance
  ----------------------------- --------------------
  Architecture and foundation                   \$20
  Frontend and visualisation                    \$35
  Audio and wake-word                           \$45
  AI and memory                                 \$40
  Automation and permissions                    \$35
  Testing and contingency                       \$25
  **Total**                                **\$200**

Use this only as a planning guide. Actual usage varies by model, context
size, tool calls, and implementation complexity.

Runtime API usage is a separate cost. Anthropic API charges are not
automatically covered by a Claude Code allowance. Keep runtime provider
costs visible and configurable.

Cost-control practices: - Ask Claude Code to inspect and plan before
large changes. - Implement one milestone or bounded task at a time. -
Avoid repeatedly feeding the entire repository when a focused context is
sufficient. - Require tests and concise implementation reports. - Do not
spend tokens polishing speculative architecture before validating the
foundation. - Keep a written decision log so both developers and model
sessions share the same decisions.

## 14. Risks and Open Decisions for Refinement

The refinement pass should explicitly investigate and resolve these
questions rather than silently assuming answers.

1.  **Wake-word accuracy:** Which engine and model can reliably detect
    "Hey JARVIS" on all target operating systems?
2.  **Voice quality:** Which British male voice is natural, low-latency,
    locally runnable, and appropriately licensed?
3.  **Audio packaging:** How will model assets be downloaded, updated,
    verified, and stored?
4.  **Cross-platform audio:** Which capture/playback library provides
    the most reliable device handling?
5.  **Provider abstraction:** What is the smallest useful interface that
    supports streaming and tool calls without overengineering?
6.  **Secrets:** Which cross-platform keychain approach should be used?
7.  **System integrations:** Which functions require native
    platform-specific code?
8.  **Browser automation:** Should Playwright run as a separate
    controlled process or be integrated in another way?
9.  **Memory policy:** Which information may be stored automatically,
    and which requires explicit user consent?
10. **Performance budgets:** Define acceptable idle CPU, memory, wake
    latency, speech latency, and hologram frame rate on average laptop
    hardware.
11. **Accessibility:** Define keyboard-only operation, screen-reader
    expectations, contrast, and reduced-motion behaviour.
12. **Updates:** Decide how application updates and model updates will
    be delivered and verified.
13. **Licensing:** Audit all code dependencies, voice models, and ML
    model licences before distribution.
14. **Packaging:** Define the release and signing strategy for Windows,
    macOS, and Linux.
15. **Offline mode:** Specify exactly what remains functional without
    internet access.

Do not add technologies solely because they are fashionable. For every
major dependency, record the reason for choosing it, alternatives
considered, platform support, maintenance status, licence, and
performance implications.

## 15. Instructions for the Refinement Pass

You are reviewing this document as a senior software architect and
product engineer.

Your job is to turn this brief into a more precise, internally
consistent, implementation-ready project specification for Claude Code.

### Required review

1.  Identify contradictions, missing requirements, unrealistic
    assumptions, and overengineering.
2.  Validate the proposed stack and recommend changes only when there is
    a concrete technical reason.
3.  Check the cross-platform implications of audio capture, wake-word
    detection, STT, TTS, keychain access, app launching, file access,
    and packaging.
4.  Define clear module boundaries and frontend/backend contracts.
5.  Specify the security model and threat boundaries in implementable
    terms.
6.  Define performance targets appropriate for average modern laptops,
    distinguishing hard requirements from aspirational goals.
7.  Clarify the first usable alpha: what it must do, what it explicitly
    will not do, and how it will be tested.
8.  Make milestones independently testable, with acceptance criteria and
    dependencies.
9.  Identify licensing, model distribution, privacy, and runtime cost
    risks.
10. Keep the architecture practical for a two-person team and the stated
    development allowance.

### Required output

Produce: - A revised, complete project specification. - A concise
architecture decision record (ADR) for each major technology choice. - A
refined repository structure. - A milestone roadmap with acceptance
tests. - A security and privacy checklist. - A list of unresolved
decisions, each with recommended experiments or decision criteria. - A
concise `CLAUDE.md` suitable for the repository root. - A first
implementation task list split between Developer A (backend) and
Developer B (frontend).

### Constraints

-   Do not implement the application during this refinement pass.
-   Do not invent that a feature has been tested.
-   Distinguish verified facts, recommendations, and open questions.
-   Prefer a small working system over a large speculative architecture.
-   Do not remove the product's defining goals: cross-platform,
    voice-first, local wake-word detection, refined British voice, blue
    holographic interface, persistent user-controlled memory, safe
    computer actions, and average-laptop performance.
-   Preserve the principle that the model can request actions but cannot
    bypass backend validation and permissions.
-   Do not turn the project into a generic web chatbot.

The final specification must be specific enough that Claude Code can
implement the first milestone without having to guess the intended
architecture or product behaviour.
