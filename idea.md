You are the lead software architect and principal engineer for a new software platform called LANPlay.

IMPORTANT:

Do NOT immediately start writing lots of code.

Your first responsibility is to design the software properly.

Think like senior engineers from Valve, NVIDIA, Discord, Microsoft, Steam, and Parsec.

Always prioritize architecture over implementation.

This project will be built incrementally.

Every phase must be fully completed, tested and validated before moving to the next phase.

If something isn't production quality, redesign it.

=========================================================
PROJECT
=========================================================

Name:

LANPlay

Mission:

Create the easiest way to play LOCAL multiplayer PC games over the Internet.

NOT a remote desktop.

NOT TeamViewer.

NOT AnyDesk.

NOT Parsec.

LANPlay is a gaming-first streaming platform.

The goal is to make remote couch co-op feel identical to sitting in front of the same PC.

Example:

Host launches FIFA 18.

Friend joins.

Friend's controller instantly appears as Xbox Controller #2.

Game thinks both players are local.

No configuration.

No networking knowledge.

Just play.

=========================================================
VISION
=========================================================

User opens LANPlay.

↓

Clicks Create Room.

↓

Receives a 6-digit room code.

↓

Friend enters room code.

↓

Automatic connection.

↓

Video appears.

↓

Friend's controller becomes Player 2.

↓

Play.

Zero router configuration.

Zero networking knowledge.

Zero VPN setup.

=========================================================
DESIGN PHILOSOPHY
=========================================================

Everything must optimize for:

Ultra-low latency.

Minimal input delay.

Minimal encoding delay.

Minimal decoding delay.

Maximum smoothness.

Gaming first.

Everything else second.

Never sacrifice controller latency for image quality.

=========================================================
PRIMARY GOAL
=========================================================

Controller latency should feel nearly local.

This is more important than image quality.

Image quality can be lowered automatically.

Controller latency should always remain excellent.

=========================================================
SUPPORTED GAMES
=========================================================

FIFA

EA FC

PES

Overcooked

Cuphead

Moving Out

Gang Beasts

LEGO Games

Castle Crashers

Any Windows local multiplayer game.

=========================================================
SUPPORTED PLATFORMS
=========================================================

Host:

Windows

Client:

Windows

Future:

Linux

Steam Deck

Android

macOS

iOS

=========================================================
TECH STACK
=========================================================

Desktop Application

Tauri

Rust backend

React

TypeScript

TailwindCSS

Framer Motion

Reason:

Small memory footprint

Native performance

Beautiful UI

=========================================================
NETWORKING
=========================================================

For V1

Use Tailscale for connectivity.

Reason:

Do NOT spend months implementing NAT traversal.

The objective is validating the streaming architecture.

Tailscale handles:

Peer discovery

Encrypted tunnel

Connectivity

Later versions will replace this with native networking.

=========================================================
V2 NETWORKING
=========================================================

Research

ICE

STUN

TURN

UDP Hole Punching

QUIC

WebRTC

libdatachannel

Pion

ENet

Reliable UDP

Congestion Control

Forward Error Correction

Packet Recovery

Jitter Buffers

Eventually LANPlay should own its networking stack.

=========================================================
VIDEO CAPTURE
=========================================================

Research

Desktop Duplication API

Windows Graphics Capture

DirectX Capture

Do NOT use GDI.

Do NOT use OBS internally.

Capture must remain GPU accelerated.

=========================================================
VIDEO ENCODING
=========================================================

Never CPU encode unless absolutely necessary.

Support

NVENC

AMF

Intel QuickSync

Codecs

H264

HEVC

AV1 (future)

Automatic bitrate adaptation.

Dynamic resolution scaling.

Frame pacing.

=========================================================
VIDEO DECODING
=========================================================

Hardware decoding.

DXVA2

D3D11

Future Vulkan.

=========================================================
AUDIO
=========================================================

Capture:

WASAPI Loopback

Codec:

Opus

Low latency.

=========================================================
CONTROLLERS
=========================================================

This is the most important subsystem.

Research:

ViGEmBus successor / virtual gamepad APIs

Virtual HID

RawInput

XInput

DirectInput

Controller hotplug

Controller remapping

Rumble

Analog triggers

Multiple controllers

Profiles

Controller synchronization

Incoming controller packets should appear exactly like locally connected Xbox controllers.

=========================================================
INPUT PIPELINE
=========================================================

Never simply send button presses.

Design an input protocol.

Each packet should contain:

Timestamp

Sequence Number

Buttons

Axes

Triggers

Controller ID

Packet Number

Latency Information

Research prediction techniques.

Research rollback.

Research packet interpolation.

Research packet compression.

Controller input should always have higher priority than video.

=========================================================
VIDEO PIPELINE
=========================================================

Design:

Capture

↓

GPU Frame

↓

Hardware Encoder

↓

Packetizer

↓

Network

↓

Receiver

↓

Decoder

↓

Renderer

↓

Display

Measure latency at every stage.

=========================================================
AUDIO PIPELINE
=========================================================

Capture

↓

Encode

↓

Packetize

↓

Network

↓

Decode

↓

Playback

=========================================================
OVERLAY
=========================================================

Create an in-game overlay.

Display:

FPS

Ping

Packet Loss

Jitter

Bitrate

Resolution

Controller latency

Video latency

Audio latency

Encode time

Decode time

Dropped Frames

Network Quality

=========================================================
BACKEND
=========================================================

Language:

Go

Responsibilities:

Authentication

Rooms

Friends

Presence

Relay Allocation

Analytics

Updates

=========================================================
DATABASE
=========================================================

PostgreSQL

Redis

=========================================================
SECURITY
=========================================================

Encrypted communication.

Device pairing.

Authentication.

Secure room joining.

Future end-to-end encryption.

=========================================================
PROJECT STRUCTURE
=========================================================

Design a modular monorepo.

Example:

apps/
    launcher
    backend
    relay

packages/
    networking
    video
    audio
    controllers
    overlay
    updater
    shared
    ui

docs/
    architecture
    ADR
    RFC
    research

prototypes/

tools/

=========================================================
DEVELOPMENT PRINCIPLES
=========================================================

Every module must be independent.

Every module must expose interfaces.

Avoid tight coupling.

Everything should be replaceable.

Networking should be replaceable.

Encoder should be replaceable.

Controller backend should be replaceable.

=========================================================
YOUR FIRST RESPONSIBILITY
=========================================================

DO NOT START CODING.

Create complete documentation.

I want:

Software Architecture

Module Diagram

Sequence Diagrams

Flowcharts

Technology Decisions

Architecture Decision Records (ADR)

Research Documents

Risks

Unknowns

Future Improvements

Trade-offs

Performance Budget

Latency Budget

Memory Budget

Bandwidth Budget

=========================================================
PHASED DEVELOPMENT
=========================================================

Create an implementation roadmap.

Each phase must have:

Objective

Deliverables

Architecture

Tasks

Testing Plan

Success Criteria

Known Risks

Documentation

Do NOT proceed to the next phase until the current one is validated.

Example:

Phase 0
Research
Architecture
Tech validation

Phase 1
Project bootstrap
CI/CD
Monorepo
Coding standards
Shared interfaces

Phase 2
Controller subsystem prototype
Virtual controller injection
Remote controller transport
Latency measurement

Phase 3
Networking abstraction
Tailscale integration
Reliable messaging
Packet protocol

Phase 4
Desktop capture prototype
Frame timing
Capture benchmarking

Phase 5
Hardware encoding
NVENC
QuickSync
AMF

Phase 6
Video streaming prototype

Phase 7
Audio streaming

Phase 8
Synchronization

Phase 9
Room system

Phase 10
Beautiful UI

Phase 11
Performance optimization

Phase 12
Packaging

Phase 13
Public alpha

=========================================================
IMPORTANT RULES
=========================================================

Never generate placeholder architecture.

Never create fake implementations.

Always justify every technical decision.

If a better technology exists, explain why.

If there are multiple options, compare them.

Whenever possible include benchmarks, latency estimates and expected performance.

Think like a principal engineer building software used by millions of gamers.

Question assumptions.

Optimize for maintainability.

Optimize for scalability.

Optimize for developer experience.

Do not rush implementation.

The architecture should be good enough that the project can continue for years.