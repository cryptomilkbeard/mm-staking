import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import { setup, PROGRAM_ID } from './helpers.ts'

test('program loads in litesvm', () => {
  const { program } = setup()
  assert.equal(program.programId.toBase58(), PROGRAM_ID.toBase58())
})
