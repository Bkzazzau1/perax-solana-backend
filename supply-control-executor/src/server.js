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
  getAssociatedTokenAddressSync,
} from '@solana/spl-token';

const PORT = Number(process.env.PORT || process.env.EXECUTOR_PORT || 8787);
const DEFAULT_RPC_URL = process.env.SOLANA_RPC_URL || 'https://api.devnet.solana.com';
const EXECUTOR_TOKEN = process.env.PERAX_SUPPLY_CONTROL_EXECUTOR_TOKEN || '';
const AUTHORITY_KEYPAIR_PATH = process.env.PERAX_AUTHORITY_KEYPAIR_PATH || '';
const TRADING_COMPANY_AUTHORITY_KEYPAIR_PATH = process.env.TRADING_COMPANY_AUTHORITY_KEYPAIR_PATH || '';

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

function encodeMarketConditionBurnParams(payload) {
  const amount = Number(payload.amountBaseUnits);
  const eligibleRevenueAmount = Number(payload.eligibleRevenueBaseUnits);
  const burnRateBps = Number(payload.burnRateBps);
  const marketHealthScore = Number(payload.marketHealthScore);
  const observedAtUnix = Number(payload.observedAtUnix);

  if (!Number.isSafeInteger(amount) || amount <= 0) {
    throw new Error('amountBaseUnits must be a positive safe integer');
  }
  if (!Number.isSafeInteger(eligibleRevenueAmount) || eligibleRevenueAmount <= 0) {
    throw new Error('eligibleRevenueBaseUnits must be a positive safe integer');
  }
  if (!Number.isInteger(burnRateBps) || burnRateBps < 0 || burnRateBps > 10000) {
    throw new Error('burnRateBps must be between 0 and 10000');
  }
  if (!Number.isInteger(marketHealthScore) || marketHealthScore < 0 || marketHealthScore > 100) {
    throw new Error('marketHealthScore must be between 0 and 100');
  }
  if (!Number.isSafeInteger(observedAtUnix) || observedAtUnix <= 0) {
    throw new Error('observedAtUnix must be a positive unix timestamp');
  }

  const decisionId = hexTo32Bytes(payload.decisionIdHex, 'decisionIdHex');

  return Buffer.concat([
    instructionDiscriminator('execute_market_condition_burn'),
    u64Le(amount),
    u64Le(eligibleRevenueAmount),
    u16Le(burnRateBps),
    Buffer.from([marketHealthScore]),
    i64Le(observedAtUnix),
    decisionId,
  ]);
}

function deriveStatePda(programId) {
  return PublicKey.findProgramAddressSync([Buffer.from('perax-state')], programId)[0];
}

function deriveBurnRecordPda(programId, decisionIdHex) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('burn'), hexTo32Bytes(decisionIdHex, 'decisionIdHex')],
    programId,
  )[0];
}

async function executeBurn(payload) {
  const rpcUrl = payload.solanaRpcUrl || DEFAULT_RPC_URL;
  const connection = new Connection(rpcUrl, 'confirmed');

  const authority = loadKeypair(AUTHORITY_KEYPAIR_PATH, 'authority');
  const tradingCompanyAuthority = loadKeypair(
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

  const tradingCompanyRevenueTokenAccount = new PublicKey(payload.tradingCompanyRevenueTokenAccount);
  const expectedRevenueAta = getAssociatedTokenAddressSync(
    tokenMint,
    tradingCompanyAuthority.publicKey,
    false,
    TOKEN_PROGRAM_ID,
  );
  if (!tradingCompanyRevenueTokenAccount.equals(expectedRevenueAta)) {
    throw new Error('tradingCompanyRevenueTokenAccount must be the ATA owned by trading company authority for PEX mint');
  }

  const burnRecordPda = deriveBurnRecordPda(programId, payload.decisionIdHex);
  const existingBurnRecord = await connection.getAccountInfo(burnRecordPda);
  if (existingBurnRecord) {
    throw new Error('burn decision already executed on-chain');
  }

  const instruction = new TransactionInstruction({
    programId,
    keys: [
      { pubkey: statePda, isSigner: false, isWritable: false },
      { pubkey: authority.publicKey, isSigner: true, isWritable: true },
      { pubkey: tradingCompanyAuthority.publicKey, isSigner: true, isWritable: false },
      { pubkey: burnRecordPda, isSigner: false, isWritable: true },
      { pubkey: tradingCompanyRevenueTokenAccount, isSigner: false, isWritable: true },
      { pubkey: tokenMint, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: encodeMarketConditionBurnParams(payload),
  });

  const transaction = new Transaction().add(instruction);
  const signature = await sendAndConfirmTransaction(
    connection,
    transaction,
    [authority, tradingCompanyAuthority],
    { commitment: 'confirmed' },
  );

  return {
    signature,
    burnRecord: burnRecordPda.toBase58(),
    authority: authority.publicKey.toBase58(),
    tradingCompanyAuthority: tradingCompanyAuthority.publicKey.toBase58(),
  };
}

const server = http.createServer(async (req, res) => {
  try {
    if (req.method === 'GET' && req.url === '/health') {
      return jsonResponse(res, 200, { ok: true, service: 'perax-supply-control-executor' });
    }

    if (req.method !== 'POST' || req.url !== '/execute/market-condition-burn') {
      return jsonResponse(res, 404, { error: 'not found' });
    }

    assertBearer(req);
    const payload = await readJsonBody(req);
    const result = await executeBurn(payload);

    return jsonResponse(res, 200, {
      accepted: true,
      signature: result.signature,
      burnRecord: result.burnRecord,
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
