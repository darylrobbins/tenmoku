-- A small counter with a testbench-style package.
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

/* Block comment:
   a package of helpers. */
package util_pkg is
  constant WIDTH : integer := 8;
  subtype byte_t is std_logic_vector(7 downto 0);
  type state_t is (idle, running, done);
  type word_array is array (0 to 3) of byte_t;
  function parity(v : std_logic_vector) return std_logic;
end package util_pkg;

package body util_pkg is
  function parity(v : std_logic_vector) return std_logic is
    variable r : std_logic := '0';
  begin
    for i in v'range loop
      r := r xor v(i);
    end loop;
    return r;
  end function parity;
end package body util_pkg;

library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;
use work.util_pkg.all;

entity counter is
  generic (
    MAX : natural := 255
  );
  port (
    clk   : in  std_logic;
    rst   : in  std_logic;
    en    : in  std_logic;
    count : out unsigned(7 downto 0)
  );
end entity counter;

architecture rtl of counter is
  signal value : unsigned(7 downto 0) := x"00";
  signal state : state_t := idle;
  constant MSG : string := "counter ""ready""";
begin
  proc : process (clk, rst)
  begin
    if rst = '1' then
      value <= (others => '0');
      state <= idle;
    elsif rising_edge(clk) then
      case state is
        when idle =>
          if en = '1' then
            state <= running;
          end if;
        when running =>
          if value = to_unsigned(MAX, 8) then
            state <= done;
          else
            value <= value + 1;
          end if;
        when others =>
          null;
      end case;
    end if;
  end process proc;

  count <= value;

  check : assert value <= 200 or state /= idle
    report "unexpected state " & MSG
    severity warning;
end architecture rtl;
