(* A small inventory: records, variants, modules, functors and printf. *)

open Printf

type colour = Red | Green | Blue | Custom of int * int * int

type item = {
  name : string;
  mutable count : int;
  price : float;
  tags : string list;
}

exception Out_of_stock of string

module type STORE = sig
  type t
  val empty : t
  val add : item -> t -> t
  val find : string -> t -> item option
end

module Store : STORE = struct
  module M = Map.Make (String)

  type t = item M.t

  let empty = M.empty
  let add item store = M.add item.name item store
  let find name store = M.find_opt name store
end

let hex_of_colour = function
  | Red -> 0xff0000
  | Green -> 0x00ff00
  | Blue -> 0x0000ff
  | Custom (r, g, b) -> (r lsl 16) lor (g lsl 8) lor b

let rec total = function
  | [] -> 0.0
  | { price; count; _ } :: rest -> (price *. float_of_int count) +. total rest

let take item n =
  if item.count < n then raise (Out_of_stock item.name)
  else item.count <- item.count - n

(** Formats an item as one line; tags are joined with ", ". *)
let describe ?(currency = "$") item =
  let tags = String.concat ", " item.tags in
  sprintf "%-10s %3d @ %s%.2f [%s]\n" item.name item.count currency item.price tags

let banner = "Inventory\t\"main\" \x41\u{1F600}\\"
let separator = '\n'
let large = 1_000_000L
let ratio = 3.14e-2

class counter start = object (self)
  val mutable n = start
  method next = n <- n + 1; self#current
  method current = n
end

let () =
  let items =
    [
      { name = "apple"; count = 12; price = 0.5; tags = [ "fruit"; "red" ] };
      { name = "pear"; count = 3; price = 0.75; tags = [ "fruit" ] };
    ]
  in
  let store = List.fold_left (fun s i -> Store.add i s) Store.empty items in
  print_string banner;
  print_char separator;
  List.iter (fun i -> print_string (describe i)) items;
  printf "total: %.2f, large: %Ld, ratio: %g\n" (total items) large ratio;
  (match Store.find "pear" store with
   | Some pear -> (
       try take pear 5 with Out_of_stock name -> eprintf "no %s left\n" name)
   | None -> failwith "missing pear");
  let c = new counter 0 in
  for _ = 1 to 3 do
    ignore (c#next)
  done;
  assert (c#current = 3 && hex_of_colour Blue > 0);
  items |> List.length |> string_of_int |> print_endline
