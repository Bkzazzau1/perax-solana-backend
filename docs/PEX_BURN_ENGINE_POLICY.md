# PEX Burn Engine Policy

## 1. Purpose

This policy defines how PEX revenue received through the Pera-X backend should be handled when users buy internal platform Credits using PEX.

The goal is to support token value, protect operating liquidity, avoid uncontrolled selling pressure, and create a clear audit trail.

## 2. Core Rule

PEX and Credits are separate balances.

```text
PEX = ecosystem token / asset
Credits = internal platform spending balance
```

When a user buys Credits using PEX:

```text
User pays PEX
Backend confirms payment
Backend credits user Credits
Backend immediately triggers the approved burn portion
Remaining PEX revenue stays in the Trading Company second wallet
Trading Company second wallet is subject to a 50% monthly selling cap
```

## 3. Trading Company Wallet Structure

The Trading Company should use two wallet layers:

### 3.1 Locked / Strategic Wallet

This wallet is for locked allocation, long-term reserve, or controlled policy holdings.

It should not be used for daily PEX revenue settlement.

### 3.2 Second / Operational Revenue Wallet

This wallet is not locked.

It receives or holds PEX revenue from user Credit purchases after the immediate burn portion is handled.

This wallet is used for operational liquidity, service settlement support, market operations, and approved company needs.

## 4. Immediate Burn Rule

When the backend credits user Credits after confirming a PEX payment, the burn engine should immediately create or execute the burn action for the approved burn portion.

This means burn should be tied to real utility activity, not random supply reduction.

```text
Credit granted = burn review/trigger happens immediately
```

The immediate burn portion should come from confirmed PEX revenue only.

The backend must not burn tokens needed for provider settlement, refunds, pending disputes, liquidity support, or operational safety.

## 5. Remaining PEX Revenue Rule

After the immediate burn portion, the remaining PEX revenue should remain in the Trading Company second wallet.

This remaining balance is not automatically sold.

It can be used only under approved operational policy.

## 6. Monthly Selling Cap

The Trading Company second wallet must not sell more than 50% of its PEX revenue balance within a monthly period.

```text
Maximum monthly sell cap = 50% of available PEX revenue in the second wallet
```

This protects the market from excessive Trading Company sell pressure.

The remaining 50% or more should stay as retained PEX reserve, liquidity support, or future strategic balance.

## 7. Sell Cap Accounting

The backend should track:

1. Monthly opening PEX revenue balance.
2. New PEX revenue received during the month.
3. Burned PEX amount.
4. PEX sold during the month.
5. Remaining monthly sell allowance.
6. Current second wallet PEX balance.

A sell action should be rejected if it exceeds the monthly allowance.

## 8. Recommended Backend Flow

```text
1. Detect confirmed PEX payment.
2. Validate payment reference.
3. Calculate Credits to grant.
4. Credit user account.
5. Calculate approved burn amount.
6. Trigger immediate burn decision/execution.
7. Record remaining PEX as Trading Company second wallet revenue.
8. Update monthly sell cap ledger.
9. Reject any sell request above 50% monthly cap.
```

## 9. Burn Execution Mode

During development and early production, burn execution should remain controlled.

```text
manual   = declare/store burn decisions only; no execution
approved = execute only approved burn decisions
```

A future production mode may support immediate execution only after wallet controls, approval rules, and smart contract integration are fully tested.

## 10. Audit Requirements

Every burn and sell-cap event must keep an audit trail:

1. User payment reference.
2. Credit amount granted.
3. PEX amount received.
4. Burn amount.
5. Remaining PEX amount.
6. Destination second wallet.
7. Sell cap month.
8. Sell amount if any.
9. Approval status.
10. Transaction signature when available.

## 11. Final Principle

PEX burns must be tied to real platform usage.

The Trading Company can operate from its second wallet, but it cannot sell more than 50% of PEX revenue monthly.

This model supports user utility, token confidence, market protection, and long-term ecosystem discipline.
