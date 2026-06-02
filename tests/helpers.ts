import { readFileSync } from 'node:fs'
import { LiteSVM } from 'litesvm'
import { LiteSVMProvider } from 'anchor-litesvm'
import { Program, BN } from '@coral-xyz/anchor'
import { PublicKey, Keypair, Transaction, SystemProgram } from '@solana/web3.js'
import {
  MINT_SIZE,
  AccountLayout,
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  createInitializeMint2Instruction,
  createMintToInstruction,
  createAssociatedTokenAccountInstruction,
  getAssociatedTokenAddressSync,
} from '@solana/spl-token'

// Load the Anchor-generated IDL via fs (Node 24 dropped `assert { type: 'json' }`).
const idl = JSON.parse(readFileSync(new URL('../target/idl/mm_staking.json', import.meta.url), 'utf8'))

export const PROGRAM_ID = new PublicKey(idl.address)
export {
  BN, PublicKey, Keypair, TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction,
}

// Module-level handle so the spl-token-shaped helpers below can reach the active svm/provider.
let CURRENT: { svm: LiteSVM; provider: any; payer: Keypair } | null = null

export function setup() {
  const svm = new LiteSVM()
  svm.addProgramFromFile(PROGRAM_ID, 'target/deploy/mm_staking.so')
  const provider = new LiteSVMProvider(svm)
  const payer = provider.wallet.payer as Keypair
  CURRENT = { svm, provider, payer }
  const program = new Program(idl as any, provider)
  return { svm, provider, program, payer }
}

export function poolPda(stakeMint: PublicKey) {
  return PublicKey.findProgramAddressSync([Buffer.from('pool'), stakeMint.toBuffer()], PROGRAM_ID)[0]
}
export function stakeVaultPda(pool: PublicKey) {
  return PublicKey.findProgramAddressSync([Buffer.from('stake_vault'), pool.toBuffer()], PROGRAM_ID)[0]
}
export function rewardVaultPda(pool: PublicKey, mint: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('reward_vault'), pool.toBuffer(), mint.toBuffer()],
    PROGRAM_ID,
  )[0]
}
export function stakerPda(pool: PublicKey, owner: PublicKey) {
  return PublicKey.findProgramAddressSync([Buffer.from('staker'), pool.toBuffer(), owner.toBuffer()], PROGRAM_ID)[0]
}

// --- LiteSVM-native token helpers ---
// LiteSVM is in-process with no RPC, so @solana/spl-token's connection-based createMint/mintTo/getAccount
// do NOT work. These wrappers mirror their signatures (the `connection` arg is ignored) but submit raw
// SPL instructions through the LiteSVM provider. Validated end-to-end against litesvm 0.3.3.

export async function createMint(
  _conn: any, payer: Keypair, mintAuthority: PublicKey, freezeAuthority: PublicKey | null, decimals: number,
): Promise<PublicKey> {
  const { svm, provider } = CURRENT!
  const mint = Keypair.generate()
  const lamports = Number(svm.minimumBalanceForRentExemption(BigInt(MINT_SIZE)))
  const tx = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey, newAccountPubkey: mint.publicKey,
      lamports, space: MINT_SIZE, programId: TOKEN_PROGRAM_ID,
    }),
    createInitializeMint2Instruction(mint.publicKey, decimals, mintAuthority, freezeAuthority),
  )
  await provider.sendAndConfirm(tx, [payer, mint])
  return mint.publicKey
}

export async function mintTo(
  _conn: any, payer: Keypair, mint: PublicKey, dest: PublicKey, authority: Keypair, amount: number | bigint,
): Promise<void> {
  const { provider } = CURRENT!
  const tx = new Transaction().add(createMintToInstruction(mint, dest, authority.publicKey, BigInt(amount)))
  await provider.sendAndConfirm(tx, [payer, authority])
}

export async function getAccount(
  _conn: any, address: PublicKey,
): Promise<{ amount: bigint; mint: PublicKey; owner: PublicKey }> {
  const { svm } = CURRENT!
  const acc = svm.getAccount(address)
  if (!acc) throw new Error('token account not found: ' + address.toBase58())
  const d = AccountLayout.decode(Buffer.from(acc.data))
  return { amount: d.amount, mint: new PublicKey(d.mint), owner: new PublicKey(d.owner) }
}

/** Advance the LiteSVM clock by `seconds` without changing slot/epoch. */
export function warp(svm: any, seconds: number) {
  const c = svm.getClock()
  c.unixTimestamp = c.unixTimestamp + BigInt(seconds)
  svm.setClock(c)
}
