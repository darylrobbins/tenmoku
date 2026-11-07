#!/usr/bin/env escript
%% -*- erlang -*-
%% A small bank-account server, touching most of the language.
-module(sample).
-behaviour(gen_server).

-export([start_link/1, deposit/2, balance/1, main/1]).
-export([init/1, handle_call/3, handle_cast/2]).
-export_type([account/0]).

-include_lib("kernel/include/file.hrl").
-define(TIMEOUT, 5000).
-define(LOG(Fmt, Args), io:format(Fmt ++ "~n", Args)).

-record(account, {id :: integer(), owner = <<"nobody">> :: binary(), funds = 0.0}).

-type account() :: #account{}.
-type result() :: {ok, number()} | {error, atom()}.

-spec start_link(binary()) -> {ok, pid()} | ignore | {error, term()}.
start_link(Owner) ->
    gen_server:start_link(?MODULE, Owner, []).

-spec deposit(pid(), number()) -> result().
deposit(Pid, Amount) when is_number(Amount), Amount > 0 ->
    gen_server:call(Pid, {deposit, Amount}, ?TIMEOUT);
deposit(_Pid, _Amount) ->
    {error, bad_amount}.

balance(Pid) -> gen_server:call(Pid, balance).

init(Owner) ->
    {ok, #account{id = erlang:unique_integer([positive]), owner = Owner}}.

handle_call({deposit, Amount}, _From, #account{funds = F} = State) ->
    New = F + Amount,
    ?LOG("deposit ~p -> ~p", [Amount, New]),
    {reply, {ok, New}, State#account{funds = New}};
handle_call(balance, _From, State) ->
    {reply, {ok, State#account.funds}, State}.

handle_cast(stop, State) ->
    {stop, normal, State};
handle_cast(_Msg, State) ->
    {noreply, State}.

classify(N) ->
    case N of
        0 -> zero;
        _ when N < 0 -> negative;
        _ -> positive
    end.

main(Args) ->
    Escapes = "tab\t newline\n quote\" backslash\\",
    Char = $a,
    Hex = 16#FF + 2#1010 + 1.5e3 + 42,
    Bin = <<1, 2, "abc", Hex:16/big>>,
    Doubled = [X * 2 || X <- lists:seq(1, 10), X rem 2 =:= 0],
    F = fun(A, B) -> A + B end,
    try
        {ok, Pid} = start_link(<<"alice">>),
        receive
            {Pid, done} -> ok
        after 100 -> timeout
        end,
        io:format("~s ~p ~p~n", [Escapes, Char, {Bin, Doubled, F(1, 2)}]),
        deposit(Pid, 10) andalso true
    catch
        error:badarg -> io:format("bad~n");
        _:_ -> ok
    end.
