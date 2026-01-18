/**
 * StateView - Direct access to Rkyv-serialized state.
 */

/**
 * A view into Rkyv-serialized state data.
 *
 * This class provides direct access to state without creating
 * JavaScript objects that would trigger garbage collection.
 *
 * @example
 * ```typescript
 * const snapshot = memio.readSharedState();
 * if (!snapshot) return;
 *
 * // Direct memory access - no object creation
 * const score = snapshot.view.readU64(SCORE_OFFSET);
 * const playerX = snapshot.view.readF32(PLAYER_X_OFFSET);
 * ```
 */
export class StateView {
  private readonly buffer: ArrayBuffer;
  private readonly dataView: DataView;
  private readonly uint8View: Uint8Array;
  private readonly offset: number;
  private readonly length: number;

  /**
   * Creates a new StateView from raw bytes.
   */
  constructor(data: ArrayBuffer | Uint8Array) {
    if (data instanceof ArrayBuffer) {
      this.buffer = data;
      this.offset = 0;
      this.length = data.byteLength;
    } else if (data instanceof Uint8Array) {
      if (typeof SharedArrayBuffer !== 'undefined' && data.buffer instanceof SharedArrayBuffer) {
        throw new TypeError("SharedArrayBuffer is not supported.");
      }
      this.buffer = data.buffer as ArrayBuffer;
      this.offset = data.byteOffset;
      this.length = data.byteLength;
    } else {
      throw new TypeError("Invalid data type. Expected ArrayBuffer or Uint8Array.");
    }
    this.dataView = new DataView(this.buffer, this.offset, this.length);
    this.uint8View = new Uint8Array(this.buffer, this.offset, this.length);
  }

  /**
   * Returns the total size of the state in bytes.
   */
  get byteLength(): number {
    return this.length;
  }

  /**
   * Returns the underlying bytes as Uint8Array.
   */
  get bytes(): Uint8Array {
    return this.uint8View;
  }

  /**
   * Returns the underlying ArrayBuffer.
   */
  get rawBuffer(): ArrayBuffer {
    return this.buffer.slice(this.offset, this.offset + this.length);
  }

  // ===== Unsigned Integer Readers =====

  /**
   * Reads an unsigned 8-bit integer at the given offset.
   */
  readU8(offset: number): number {
    return this.dataView.getUint8(offset);
  }

  /**
   * Reads an unsigned 16-bit integer at the given offset (little-endian).
   */
  readU16(offset: number): number {
    return this.dataView.getUint16(offset, true);
  }

  /**
   * Reads an unsigned 32-bit integer at the given offset (little-endian).
   */
  readU32(offset: number): number {
    return this.dataView.getUint32(offset, true);
  }

  /**
   * Reads an unsigned 64-bit integer at the given offset (little-endian).
   * Returns a BigInt since JS numbers can't represent all u64 values.
   */
  readU64(offset: number): bigint {
    return this.dataView.getBigUint64(offset, true);
  }

  /**
   * Reads an unsigned 64-bit integer as a number (loses precision for large values).
   */
  readU64AsNumber(offset: number): number {
    return Number(this.readU64(offset));
  }

  // ===== Signed Integer Readers =====

  /**
   * Reads a signed 8-bit integer at the given offset.
   */
  readI8(offset: number): number {
    return this.dataView.getInt8(offset);
  }

  /**
   * Reads a signed 16-bit integer at the given offset (little-endian).
   */
  readI16(offset: number): number {
    return this.dataView.getInt16(offset, true);
  }

  /**
   * Reads a signed 32-bit integer at the given offset (little-endian).
   */
  readI32(offset: number): number {
    return this.dataView.getInt32(offset, true);
  }

  /**
   * Reads a signed 64-bit integer at the given offset (little-endian).
   */
  readI64(offset: number): bigint {
    return this.dataView.getBigInt64(offset, true);
  }

  // ===== Float Readers =====

  /**
   * Reads a 32-bit float at the given offset (little-endian).
   */
  readF32(offset: number): number {
    return this.dataView.getFloat32(offset, true);
  }

  /**
   * Reads a 64-bit float at the given offset (little-endian).
   */
  readF64(offset: number): number {
    return this.dataView.getFloat64(offset, true);
  }

  // ===== String/Bytes Readers =====

  /**
   * Reads a UTF-8 string of the given length at the offset.
   *
   * Note: This creates a new string object (GC pressure).
   * For frequent reads, consider caching or using typed arrays.
   */
  readString(offset: number, length: number): string {
    const bytes = this.uint8View.slice(offset, offset + length);
    return new TextDecoder().decode(bytes);
  }

  /**
   * Reads a slice of bytes at the given offset.
   *
   * Returns a view (no copy) - modifications affect the original.
   */
  readBytes(offset: number, length: number): Uint8Array {
    return this.uint8View.subarray(offset, offset + length);
  }

  /**
   * Reads a slice of bytes and returns a copy.
   */
  readBytesCopy(offset: number, length: number): Uint8Array {
    return this.uint8View.slice(offset, offset + length);
  }

  // ===== Rkyv-specific helpers =====

  /**
   * Reads a Rkyv relative pointer and returns the absolute offset.
   *
   * Rkyv uses relative pointers (i32) to reference other parts of the archive.
   */
  resolveRelativePtr(offset: number): number {
    const relPtr = this.readI32(offset);
    return offset + relPtr;
  }

  /**
   * Reads a Rkyv ArchivedString.
   *
   * ArchivedString layout: [relative_ptr: i32, len: u32]
   */
  readArchivedString(offset: number): string {
    const dataOffset = this.resolveRelativePtr(offset);
    const length = this.readU32(offset + 4);
    return this.readString(dataOffset, length);
  }

  /**
   * Reads a Rkyv ArchivedVec length.
   *
   * ArchivedVec layout: [relative_ptr: i32, len: u32]
   */
  readArchivedVecLen(offset: number): number {
    return this.readU32(offset + 4);
  }

  /**
   * Returns the data pointer offset for a Rkyv ArchivedVec.
   */
  readArchivedVecPtr(offset: number): number {
    return this.resolveRelativePtr(offset);
  }
}
