import http from 'node:http';
import fs from 'node:fs';
import crypto from 'node:crypto';
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from '@solana/web3.js';
import {
  TOKEN_PROGRAM_ID,
  getAccount,
} from '@solana/spl-token';

loadDotenv();

const PORT = Number(process.env.PORT || process.env.EXECUTOR_PORT || 8787);
const DEFAULT_RPC_URL = process.env.SOLANA_RPC_URL || 'https://api.devnet.solana.com';
const EXECUTOR_TOKEN = process.env.PERAX_SUPPLY_CONTROL_EXECUTOR_TOKEN || '';
const AUTHORITY_KEYPAIR_PATH = process.env.PERAX_AUTHORITY_KEYPAIR_PATH || '';
const TRADING_COMPANY_AUTHORITY_KEYPAIR_PATH = process.env.TRADING_COMPANY_AUTHORITY_KEYPAIR_PATH || '';
const TRADING_COMPANY_TOKEN_ACCOUNT = process.env.TRADING_COMPANY_TOKEN_ACCOUNT || process.env.TRADING_CO_TREASURY || '';
const TRADING_COMPANY_REVENUE_TOKEN_ACCOUNT = process.env.TRADING_COMPANY_REVENUE_TOKEN_ACCOUNT || process.env.TRADING_COMPANY_SECOND_WALLET || '';

function loadDotenv() {
  const envUrl = new URL('../.env', import.meta.url);
  if (!fs.existsSync(envUrl)) {
    return;
  }

  const lines = fs.readFileSync(envUrl, 'utf8').split(/\r?\n/);
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) {
      continue;
    }

    const separatorIndex = trimmed.indexOf('=');
    if (separatorIndex <= 0) {
      continue;
    }

    const key = trimmed.slice(0, separatorIndex).trim();
    let value = trimmed.slice(separatorIndex + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }

    process.env[key] ??= value;
  }
}

function jsonResponse(res, status, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(body),
  });
  res.end(body);
}

function readJsonBody(req) {
  return new Promise((resolve, reject) => {
    let body = '';
    req.on('data', (chunk) => {
      body += chunk;
      if (body.length > 1024 * 1024) {
        reject(new Error('request body too large'));
        req.destroy();
      }
    });
    req.on('end', () => {
      try {
        resolve(body ? JSON.parse(body) : {});
      } catch (error) {
        reject(new Error('invalid JSON body'));
      }
    });
    req.on('error', reject);
  });
}

function assertBearer(req) {
  if (!EXECUTOR_TOKEN) {
    return;
  }
  const header = req.headers.authorization || '';
  const expected = `Bearer ${EXECUTOR_TOKEN}`;
  if (header !== expected) {
    throw new Error('unauthorized executor request');
  }
}

function loadKeypair(path, label) {
  if (!path) {
    throw new Error(`${label} keypair path is required`);
  }
  const raw = fs.readFileSync(path, 'utf8');
  const secret = JSON.parse(raw);
  if (!Array.isArray(secret)) {
    throw new Error(`${label} keypair file must contain a JSON secret-key array`);
  }
  return Keypair.fromSecretKey(Uint8Array.from(secret));
}

function u64Le(value) {
  const buffer = Buffer.alloc(8);
  buffer.writeBigUInt64LE(BigInt(value));
  return buffer;
}

function u16Le(value) {
  const buffer = Buffer.alloc(2);
  buffer.writeUInt16LE(Number(value));
  return buffer;
}

function u8(value) {
  return Buffer.from([Number(value)]);
}

function i64Le(value) {
  const buffer = Buffer.alloc(8);
  buffer.writeBigInt64LE(BigInt(value));
  return buffer;
}

function hexTo32Bytes(hexValue, label) {
  const clean = String(hexValue || '').trim().replace(/^0x/i, '').toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(clean)) {
    throw new Error(`${label} must be 32 bytes / 64 hex characters`);
  }
  return Buffer.from(clean, 'hex');
}

function instructionDiscriminator(name) {
  return crypto.createHash('sha256').update(`global:${name}`).digest().subarray(0, 8);
}

function toPositiveSafeInteger(value, label) {
  const numberValue = Number(value);
  if (!Number.isSafeInteger(numberValue) || numberValue <= 0) {
    throw new Error(`${label} must be a positive safe integer`);
  }
  return numberValue;
}

function toBps(value, label) {
  const numberValue = Number(value);
  if (!Number.isInteger(numberValue) || numberValue < 0 || numberValue > 10000) {
    throw new Error(`${label} must be an integer between 0 and 10000`);
  }
  return numberValue;
}

function toMarketHealthScore(value) {
  const numberValue = Number(value);
  if (!Number.isInteger(numberValue) || numberValue < 0 || numberValue > 100) {
    throw new Error('marketHealthScore must be an integer between 0 and 100');
  }
  return numberValue;
}

function toObservedAt(value) {
  const numberValue = Number(value);
  if (!Number.isSafeInteger(numberValue) || numberValue <= 0) {
    throw new Error('observedAtUnix must be a positive unix timestamp');
  }
  return numberValue;
}

function normalizeBurnSource(value) {
  const normalized = String(value || 'OpenMarketPurchase').trim().toLowerCase();
  if (['open_market_purchase', 'openmarketpurchase', 'revenue', 'revenue_account'].includes(normalized)) {
    return { label: 'OpenMarketPurchase', enumIndex: 0 };
  }
  if (['trading_treasury', 'tradingtreasury', 'treasury', 'locked'].includes(normalized)) {
    return { label: 'TradingTreasury', enumIndex: 1 };
  }
  throw new Error('burnSource must be OpenMarketPurchase or TradingTreasury');
}

function encodeConditionalBuybackBurnParams(payload) {
  const amount = toPositiveSafeInteger(payload.amountBaseUnits, 'amountBaseUnits');
  const eligibleRevenueAmount = toPositiveSafeInteger(payload.eligibleRevenueBaseUnits, 'eligibleRevenueBaseUnits');
  const burnRateBps = toBps(payload.burnRateBps, 'burnRateBps');
  const marketHealthScore = toMarketHealthScore(payload.marketHealthScore);
  const observedAt = toObservedAt(payload.observedAtUnix);
  const decisionId = hexTo32Bytes(payload.decisionIdHex, 'decisionIdHex');
  const burnSource = normalizeBurnSource(payload.burnSource);

  return Buffer.concat([
    instructionDiscriminator('execute_conditional_buyback_burn'),
    u64Le(amount),
    u64Le(eligibleRevenueAmount),
    u16Le(burnRateBps),
    u8(marketHealthScore),
    i64Le(observedAt),
    decisionId,
    u8(burnSource.enumIndex),
  ]);
}

function deriveStatePda(programId) {
  return PublicKey.findProgramAddressSync([Buffer.from('perax-state')], programId)[0];
}

function deriveBurnRecordPda(programId, decisionIdHex) {
  const decisionId = hexTo32Bytes(decisionIdHex, 'decisionIdHex');
  return PublicKey.findProgramAddressSync([Buffer.from('burn'), decisionId], programId)[0];
}

function sourceTokenAccountForBurnSource(payload, burnSource) {
  if (burnSource.label === 'OpenMarketPurchase') {
    return payload.tradingCompanyRevenueTokenAccount || TRADING_COMPANY_REVENUE_TOKEN_ACCOUNT;
  }
  return payload.tradingCompanyTokenAccount || TRADING_COMPANY_TOKEN_ACCOUNT;
}

async function executeBurn(payload) {
  const rpcUrl = payload.solanaRpcUrl || DEFAULT_RPC_URL;
  const connection = new Connection(rpcUrl, 'confirmed');

  const authority = loadKeypair(AUTHORITY_KEYPAIR_PATH, 'authority');
  const sourceAuthority = loadKeypair(
    TRADING_COMPANY_AUTHORITY_KEYPAIR_PATH,
    'trading company authority',
  );

  const programId = new PublicKey(payload.programId);
  const tokenMint = new PublicKey(payload.pexMintAddress);
  const statePda = payload.statePda ? new PublicKey(payload.statePda) : deriveStatePda(programId);
  const derivedStatePda = deriveStatePda(programId);
  if (!statePda.equals(derivedStatePda)) {
    throw new Error('statePda does not match derived perax-state PDA');
  }

  const burnSource = normalizeBurnSource(payload.burnSource);
  const sourceTokenAccountValue = sourceTokenAccountForBurnSource(payload, burnSource);
  if (!sourceTokenAccountValue) {
    throw new Error(
      burnSource.label === 'OpenMarketPurchase'
        ? 'tradingCompanyRevenueTokenAccount is required for OpenMarketPurchase burns'
        : 'tradingCompanyTokenAccount is required for TradingTreasury burns',
    );
  }

  const sourceTokenAccount = new PublicKey(sourceTokenAccountValue);
  const sourceTokenAccountState = await getAccount(
    connection,
    sourceTokenAccount,
    'confirmed',
    TOKEN_PROGRAM_ID,
  );

  if (!sourceTokenAccountState.owner.equals(sourceAuthority.publicKey)) {
    throw new Error('source token account owner does not match trading company authority');
  }

  if (!sourceTokenAccountState.mint.equals(tokenMint)) {
    throw new Error('source token account mint does not match PEX mint');
  }

  if (sourceTokenAccountState.amount < BigInt(payload.amountBaseUnits)) {
    throw new Error('source token account balance is lower than requested burn amount');
  }

  const burnRecordPda = deriveBurnRecordPda(programId, payload.decisionIdHex);

  const instruction = new TransactionInstruction({
    programId,
    keys: [
      { pubkey: statePda, isSigner: false, isWritable: true },
      { pubkey: authority.publicKey, isSigner: true, isWritable: true },
      { pubkey: sourceAuthority.publicKey, isSigner: true, isWritable: false },
      { pubkey: burnRecordPda, isSigner: false, isWritable: true },
      { pubkey: sourceTokenAccount, isSigner: false, isWritable: true },
      { pubkey: tokenMint, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: encodeConditionalBuybackBurnParams(payload),
  });

  const transaction = new Transaction().add(instruction);
  const signature = await sendAndConfirmTransaction(
    connection,
    transaction,
    [authority, sourceAuthority],
    { commitment: 'confirmed' },
  );

  return {
    signature,
    burnRecord: burnRecordPda.toBase58(),
    burnSource: burnSource.label,
    sourceTokenAccount: sourceTokenAccount.toBase58(),
    authority: authority.publicKey.toBase58(),
    tradingCompanyAuthority: sourceAuthority.publicKey.toBase58(),
  };
}

const server = http.createServer(async (req, res) => {
  try {
    if (req.method === 'GET' && req.url === '/health') {
      return jsonResponse(res, 200, { ok: true, service: 'perax-supply-control-executor' });
    }

    const supportedPaths = new Set([
      '/execute/market-condition-burn',
      '/execute/conditional-buyback-burn',
    ]);

    if (req.method !== 'POST' || !supportedPaths.has(req.url)) {
      return jsonResponse(res, 404, { error: 'not found' });
    }

    assertBearer(req);
    const payload = await readJsonBody(req);
    const result = await executeBurn(payload);

    return jsonResponse(res, 200, {
      accepted: true,
      signature: result.signature,
      burnRecord: result.burnRecord,
      burnSource: result.burnSource,
      sourceTokenAccount: result.sourceTokenAccount,
      authority: result.authority,
      tradingCompanyAuthority: result.tradingCompanyAuthority,
    });
  } catch (error) {
    return jsonResponse(res, 400, {
      accepted: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
});

server.listen(PORT, () => {
  console.log(`Pera-X supply-control executor listening on :${PORT}`);
});
