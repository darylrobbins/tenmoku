#!/usr/bin/env bb
;; A small inventory tracker.
(ns inventory.core
  (:require [clojure.string :as str]
            [clojure.set :refer [union]])
  (:import (java.util Date UUID)))

(def ^:private max-items 1000)
(def tax-rate 0.0825)
(def hex-mask 0xFF)
(def ratio 22/7)

(defrecord Item [id name price qty])

(defprotocol Priced
  (total [this] "Total price of the item"))

(extend-type Item
  Priced
  (total [{:keys [price qty]}]
    (* price qty)))

(defn make-item
  "Builds an item with a fresh id."
  [^String name price qty]
  (->Item (str (UUID/randomUUID)) name price qty))

(defmulti describe :kind)

(defmethod describe :tool [item]
  (str "Tool: " (:name item) "\t(" (:qty item) " left)\n"))

(defmacro unless [test & body]
  `(when-not ~test ~@body))

(defn- bad-qty? [qty]
  (or (nil? qty) (neg? qty) (> qty max-items)))

(defn restock
  [inventory id n]
  (let [item (get inventory id)]
    (cond
      (nil? item) (throw (ex-info "No such item" {:id id}))
      (bad-qty? n) (do (println "Bad quantity" n) inventory)
      :else (update-in inventory [id :qty] + n))))

(defn summarize [items]
  (->> items
       (map #(str (:name %) ": " (:qty %)))
       (filter #(re-find #"^[A-Z]" %))
       (str/join ", ")))

(def sample-data
  {:hammer {:name "Hammer" :price 12.5 :qty 10 :kind :tool}
   :nails  {:name "Nails \"bulk\"" :price 0.05 :qty 5000 :kind :part}
   :tags   #{:hardware :home}
   :ch     \a})

(comment
  (summarize (vals sample-data)))

#_(println "disabled")

(defn -main [& args]
  (try
    (doseq [[k v] sample-data]
      (println k (describe v)))
    (catch Exception e
      (println "failed:" (.getMessage e)))
    (finally
      (println (Date.) @(future (+ 1 2))))))

(def double-all #(map (fn [x] (* 2 x)) %))
(def tagged-all #(if (seq %) (str "has " (count %)) (do nil)))
#_(when true (let [a 1] (str "skipped " a :kw 42)))
