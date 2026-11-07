{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE QuasiQuotes #-}

{- |
Module      : Inventory
Description : A small stock-keeping example.
-}
module Inventory
  ( Item (..)
  , Stock
  , restock
  , main
  ) where

import qualified Data.Map.Strict as Map
import Data.List (sortOn)
import Control.Exception (bracket_)
import Database.PostgreSQL.Simple.SqlQQ (sql)

-- | One line of the inventory.
data Item = Item
  { itemName  :: String
  , itemPrice :: Double
  , itemCount :: !Int
  } deriving (Show, Eq)

newtype Sku = Sku Int
  deriving (Show, Eq, Ord)

type Stock = Map.Map Sku Item

class Priced a where
  price :: a -> Double

instance Priced Item where
  price item = itemPrice item * fromIntegral (itemCount item)

-- | Adds @n@ to an item's count, or leaves the stock alone.
restock :: Sku -> Int -> Stock -> Stock
restock sku n = Map.adjust bump sku
  where
    bump item = item { itemCount = itemCount item + n }

total :: Stock -> Double
total = sum . map price . Map.elems

describe :: Item -> String
describe Item { itemName = name, itemCount = count }
  | count == 0 = name ++ ": sold out\n"
  | count < 10 = name ++ ": only " ++ show count ++ " left\t(!)"
  | otherwise = name ++ ": in stock"

grade :: Double -> Char
grade value =
  case compare value 1.5e3 of
    GT -> 'A'
    EQ -> '\''
    LT -> if value > 0x1F then 'B' else 'C'

cheapest :: Stock -> [Item]
cheapest = take 3 . sortOn itemPrice . Map.elems

lowStockQuery :: String
lowStockQuery = [sql| SELECT name, count FROM items WHERE count < 10 |]

initial :: Stock
initial = Map.fromList
  [ (Sku 1, Item "widget" 2.50 4)
  , (Sku 2, Item "gadget" 10.0 0)
  , (Sku 3, Item "doohickey" 0.75 120)
  ]

main :: IO ()
main = bracket_ (putStrLn "opening") (putStrLn "closing") $ do
  let stock = restock (Sku 2) 5 initial
      names = [itemName i | i <- Map.elems stock, itemCount i > 0]
  mapM_ (putStr . describe) (Map.elems stock)
  print (total stock, grade (total stock))
  print (map itemName (cheapest stock))
  print names
  putStrLn lowStockQuery
