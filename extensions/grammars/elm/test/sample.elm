module Main exposing (Model, Msg(..), main, update, view)

{-| A small counter with a list of notes.

{- Block comments nest. -}
-}

import Browser
import Html exposing (Html, button, div, text)
import Html.Events exposing (onClick)


-- MODEL


type alias Model =
    { count : Int
    , notes : List String
    , ratio : Float
    }


type Msg
    = Increment
    | Decrement
    | AddNote String
    | Reset


init : Model
init =
    { count = 0, notes = [], ratio = 0.5 }



-- UPDATE


update : Msg -> Model -> Model
update msg model =
    case msg of
        Increment ->
            { model | count = model.count + 1 }

        Decrement ->
            { model | count = model.count - 1 }

        AddNote note ->
            { model | notes = note :: model.notes }

        Reset ->
            init


greeting : String -> String
greeting name =
    "Hello, " ++ name ++ "!\n\tTab and \"quote\" and \u{0041}"


initial : Char
initial =
    'x'


hex : Int
hex =
    0xFF + 1000 * 2


scale : Float -> Float
scale x =
    let
        factor =
            1.5e3

        offset =
            2.0
    in
    if x > 0 then
        x * factor + offset

    else
        negate x



-- VIEW


view : Model -> Html Msg
view model =
    div []
        [ button [ onClick Decrement ] [ text "-" ]
        , div [] [ text (String.fromInt model.count) ]
        , button [ onClick Increment ] [ text "+" ]
        , div [] (List.map (\n -> text n) model.notes)
        ]


main : Program () Model Msg
main =
    Browser.sandbox { init = init, update = update, view = view }
