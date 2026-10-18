/**
 * RhpExecutionClient Interface & Stub Implementation
 *
 * This client provides the abstraction layer for running RHP code.
 * In this client-side teaser, code is evaluated against a deterministic simulated VM
 * that tracks outputs, allocations, and execution timings.
 *
 * To wire up a real WebAssembly (WASM) build of the RHP compiler or a remote runner API:
 * 1. Implement this interface in your WASM loader (e.g. `wasmRhpClient.ts`)
 * 2. Replace the default instance in `getExecutionClient()`
 */

export interface RhpExecutionResult {
  stdout: string;
  stderr?: string;
  exitCode: number;
  durationMs: number;
  memoryUsageBytes: number;
  gcPauseMs: number;
  cowDetachments: number;
}

export interface RhpExecutionClient {
  run(code: string): Promise<RhpExecutionResult>;
}

/**
 * Deterministic Client-side Simulator Stub
 */
class SimulatedRhpExecutionClient implements RhpExecutionClient {
  async run(code: string): Promise<RhpExecutionResult> {
    const startTime = performance.now();

    // Small simulated compile / VM execution delay (25-45ms)
    await new Promise((resolve) => setTimeout(resolve, 35));

    let output = '';
    let memoryBytes = 1024;
    let cowCount = 0;

    // Pattern matching simple demo snippets
    if (code.includes('Arr::list')) {
      output += "[RHP VM] Evaluated Arr::list collection pipeline\n";
      output += "Filtered collection: [ 108, 961.2 ]\n";
      output += "Total items processed: 5\n";
      memoryBytes = 2480;
      cowCount = 1;
    } else if (code.includes('Future::wait')) {
      output += "[Tokio Fiber Pool] Dispatched 2 concurrent tasks\n";
      output += "[HTTP] GET https://api.internal/metrics -> 200 OK (12ms)\n";
      output += "[DB] Users::find(42) -> UserRecord { id: 42, role: 'admin' } (2ms)\n";
      output += "Aggregated response dispatched in 14.2ms\n";
      memoryBytes = 3200;
    } else if (code.includes('Memory::scope') || code.includes('Memory::gcPauseMs')) {
      output += "[Memory Subsystem] Executing in scoped RequestArena\n";
      output += "Peak memory: 524,288 bytes\n";
      output += "GC runs: 0\n";
      output += "GC pause duration: 0.000 ms\n";
      output += "Scope dropped deterministically via RAII.\n";
      memoryBytes = 512;
    } else {
      output += "Hello, World from RHP!\n";
      output += "Execution runtime: Register Bytecode VM (Cranelift JIT ready)\n";
    }

    const duration = Math.round((performance.now() - startTime) * 10) / 10;

    return {
      stdout: output,
      exitCode: 0,
      durationMs: duration,
      memoryUsageBytes: memoryBytes,
      gcPauseMs: 0.0,
      cowDetachments: cowCount,
    };
  }
}

// Export singleton instance
export const defaultExecutionClient: RhpExecutionClient = new SimulatedRhpExecutionClient();
