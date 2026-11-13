// A small FIFO with a self-checking testbench.
`timescale 1ns / 1ps
`default_nettype none
`define WIDTH 8

package fifo_pkg;
  typedef enum logic [1:0] {IDLE, PUSH, POP, FULL} state_t;

  typedef struct packed {
    logic [7:0] data;
    logic       valid;
  } entry_t;

  /* Returns the number of bits needed to count to n. */
  function automatic int clog2(input int n);
    int bits = 0;
    while ((1 << bits) < n) bits++;
    return bits;
  endfunction
endpackage

module fifo #(
  parameter int DEPTH = 16,
  parameter int WIDTH = 8,
  parameter int AW    = 4  // log2(DEPTH)
) (
  input  wire              clk,
  input  wire              rst_n,
  input  wire              push,
  input  wire              pop,
  input  wire [WIDTH-1:0]  din,
  output logic [WIDTH-1:0] dout,
  output logic             empty,
  output logic             full
);
  logic [WIDTH-1:0] mem [DEPTH];
  logic [AW:0] wr_ptr, rd_ptr;
  real ratio = 0.5;
  real tol   = 2.5e-3;

  assign empty = (wr_ptr == rd_ptr);
  assign full  = (wr_ptr[AW] != rd_ptr[AW]) && (wr_ptr[AW-1:0] == rd_ptr[AW-1:0]);

  always_ff @(posedge clk or negedge rst_n) begin : write_side
    if (!rst_n) begin
      wr_ptr <= '0;
    end else if (push && !full) begin
      mem[wr_ptr[AW-1:0]] <= din;
      wr_ptr <= wr_ptr + 1'b1;
    end
  end

  always_ff @(posedge clk or negedge rst_n) begin : read_side
    if (!rst_n) begin
      rd_ptr <= {(AW + 1){1'b0}};
      dout   <= 8'hFF;
    end else if (pop && !empty) begin
      dout   <= mem[rd_ptr[AW-1:0]];
      rd_ptr <= rd_ptr + 1;
    end
  end
endmodule

module fifo_tb;
  import fifo_pkg::*;

  localparam int N = 4;

  logic clk = 0;
  logic rst_n, push, pop;
  logic [7:0] din, dout;
  logic empty, full;
  int errors = 0;
  int aw = clog2(N);

  fifo #(.DEPTH(N), .WIDTH(8), .AW(2)) dut (
    .clk(clk), .rst_n(rst_n), .push(push), .pop(pop),
    .din(din), .dout(dout), .empty(empty), .full(full)
  );

  always #5 clk = ~clk;

  function automatic logic [7:0] pattern(input int i);
    return 8'd10 + i;
  endfunction

  initial begin
    rst_n = 0;
    push  = 0;
    pop   = 0;
    #12.5ns rst_n = 1;
    for (int i = 0; i < N; i++) begin
      @(negedge clk);
      push = 1;
      din  = pattern(i);
    end
    @(negedge clk);
    push = 0;
    if (!full) begin
      $display("error: \"full\" not set after %0d writes\n", N);
      errors++;
    end
    case (errors)
      0:       $display("PASS\t%s (aw = %0d)", "fifo", aw);
      default: $display("FAIL\t%0d errors", errors);
    endcase
    $finish;
  end
endmodule
